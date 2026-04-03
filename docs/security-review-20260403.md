# LLMPool 安全审查报告

**审查日期：** 2026-04-03  
**审查范围：** 全量代码库（`crates/llmpool`、`crates/llmpool-ctl`、数据库迁移、Docker 配置）  
**审查方法：** 人工代码审查

---

## 目录

1. [总体评估](#1-总体评估)
2. [认证与授权](#2-认证与授权)
3. [密钥与敏感数据管理](#3-密钥与敏感数据管理)
4. [数据库安全](#4-数据库安全)
5. [API 安全](#5-api-安全)
6. [Redis 缓存安全](#6-redis-缓存安全)
7. [代理与上游请求安全](#7-代理与上游请求安全)
8. [资金与余额安全](#8-资金与余额安全)
9. [配置与部署安全](#9-配置与部署安全)
10. [依赖安全](#10-依赖安全)
11. [日志与可观测性安全](#11-日志与可观测性安全)
12. [问题汇总与优先级](#12-问题汇总与优先级)
13. [修复建议](#13-修复建议)

---

## 1. 总体评估

LLMPool 是一个 OpenAI API 代理池服务，整体安全设计较为合理，使用了参数化查询防止 SQL 注入、AES-256-GCM 加密存储上游 API Key、JWT 认证管理端 API 等良好实践。但仍存在若干中高风险问题，需要在生产部署前修复。

| 风险等级 | 数量 |
|---------|------|
| 🔴 高危  | 5    |
| 🟡 中危  | 7    |
| 🟢 低危  | 6    |

---

## 2. 认证与授权

### 2.1 🔴 高危：JWT 令牌未验证过期时间

**文件：** `crates/llmpool/src/middlewares/admin_auth.rs`

```rust
// Allow tokens without exp claim for flexibility
validation.required_spec_claims.remove("exp");
validation.validate_exp = false;
```

管理端 JWT 中间件显式禁用了过期时间验证（`validate_exp = false`），且不要求 `exp` 字段。这意味着一旦 JWT 令牌泄露，攻击者可以永久使用该令牌访问所有管理 API，无法通过令牌过期机制撤销访问权限。

**建议：**
- 启用 `validate_exp = true`，要求所有 JWT 必须包含 `exp` 字段
- 设置合理的令牌有效期（如 1 小时）
- 提供令牌刷新机制

---

### 2.2 🔴 高危：OpenAI 代理端缺少速率限制

**文件：** `crates/llmpool/src/views/openai_proxy/mod.rs`、`crates/llmpool/src/server.rs`

整个 OpenAI 代理路由（`/openai/v1/*`）没有任何速率限制中间件。攻击者可以使用有效的 API Key 发起大量请求，导致：
- 账户余额被快速耗尽（DoS 攻击）
- 上游 API 配额被滥用
- 服务器资源耗尽

**建议：**
- 在 OpenAI 代理路由层添加基于 API Key 的速率限制（如使用 `tower` 的 rate-limit 中间件）
- 在 Redis 中维护每个 API Key 的请求计数器，实现滑动窗口限流

---

### 2.3 🟡 中危：API Key 认证存在缓存一致性问题

**文件：** `crates/llmpool/src/views/openai_proxy/helpers.rs`

当 Redis 缓存命中时，代码仍会再次查询数据库验证 API Key：

```rust
if let Some(info) = cached_info {
    // 即使缓存命中，仍然查询数据库
    let access_key = match db::api::find_active_api_credential_by_apikey(&state.pool, token).await {
```

这导致缓存的意义大打折扣——每次请求仍需一次数据库查询。更重要的是，缓存中的 `is_active` 状态可能与数据库不一致（TTL 15 分钟），被停用的 API Key 在缓存过期前仍可通过缓存校验。

**建议：**
- 缓存命中时，仅使用缓存数据进行验证，不再查询数据库（或仅在缓存显示 inactive 时才查库）
- 停用 API Key 时，主动删除 Redis 缓存（已有 `delete_apikey` 调用，但需确保所有停用路径都触发）

---

### 2.4 🟡 中危：管理端 API 无 IP 白名单或网络隔离

**文件：** `crates/llmpool/src/server.rs`

管理端 API（`/api/v1/*`）与用户端 OpenAI 代理（`/openai/v1/*`）绑定在同一端口（默认 `0.0.0.0:19324`），仅通过 JWT 区分。如果 JWT Secret 泄露，攻击者可以完全控制系统（创建账户、充值、管理上游等）。

**建议：**
- 将管理端 API 绑定到独立端口或仅监听内网地址
- 在反向代理层（如 Nginx）对管理端路径添加 IP 白名单

---

### 2.5 🟢 低危：API Key 生成使用 UUIDv7（时间有序）

**文件：** `crates/llmpool/src/db/api.rs`

```rust
fn generate_api_credential() -> String {
    let uuid = Uuid::now_v7();
    let hex_string = uuid.simple().to_string();
    format!("lpx-{}", hex_string)
}
```

UUIDv7 是时间有序的，前缀部分（高位）包含时间戳，理论上可以推断 API Key 的生成时间范围，但由于低位仍有足够随机性（74 位），实际暴力破解风险极低。

**建议：** 可考虑改用 `Uuid::new_v4()`（纯随机）或使用 `rand::random::<[u8; 32]>()` 生成更高熵的 API Key。

---

## 3. 密钥与敏感数据管理

### 3.1 🔴 高危：加密密钥为可选配置，默认明文存储

**文件：** `llmpool.toml.example`、`crates/llmpool/src/crypto.rs`

```toml
[security]
# If left empty, API keys will be stored in plaintext (not recommended for production).
encryption_key = ""
```

```rust
pub fn encrypt_if_configured(plaintext: &str) -> Result<String, CryptoError> {
    if is_encryption_configured() {
        encrypt(plaintext)
    } else {
        Ok(plaintext.to_string())  // 明文返回！
    }
}
```

上游 API Key 的加密是可选的。若管理员未配置 `encryption_key`，所有上游 API Key 将以明文存储在 PostgreSQL 数据库中。一旦数据库被攻破，所有上游服务的 API Key 将全部泄露。

**建议：**
- 将加密密钥设为**必填项**，启动时若未配置则拒绝启动（`panic` 或返回错误）
- 在文档中明确标注这是生产环境必须配置的安全项

---

### 3.2 🟡 中危：JWT Secret 强度无校验

**文件：** `crates/llmpool/src/middlewares/admin_auth.rs`、`crates/llmpool/src/config.rs`

`jwt_secret` 字段没有最小长度或复杂度校验。示例配置中使用了 `"your-jwt-secret-here"` 这样的弱密钥，若管理员直接使用示例配置，JWT 将极易被暴力破解。

**建议：**
- 启动时校验 `jwt_secret` 长度（建议至少 32 字节）
- 在文档中提供生成强密钥的命令：`openssl rand -base64 32`

---

### 3.3 🟡 中危：数据库连接 URL 包含明文密码

**文件：** `llmpool.toml.example`、`docker/docker-compose.yml`

```toml
url = "postgres://user:password@localhost/llmpool"
```

```yaml
DATABASE_URL: postgres://llmpool:llmpool@postgres/llmpool
```

数据库密码以明文形式存在于配置文件和 Docker Compose 文件中。Docker Compose 中使用了弱密码 `llmpool`。

**建议：**
- 使用环境变量或 Secret 管理工具（如 Vault、Docker Secrets）注入数据库密码
- Docker Compose 中使用 `.env` 文件管理敏感变量，并将 `.env` 加入 `.gitignore`

---

### 3.4 🟢 低危：`ellipsed_api_key` 字段存储在数据库中

**文件：** `crates/llmpool/migrations/20260319000001_initial_schema.sql`

```sql
ellipsed_api_key VARCHAR NOT NULL DEFAULT '',
```

`ellipsed_api_key`（如 `sk-abc...xyz`）存储在数据库中，但实际上它是在读取时动态计算的（见 `db/openai.rs` 的 `decrypt_upstream`）。数据库中的该字段始终为空字符串，存在数据不一致的风险，且增加了不必要的存储。

**建议：** 将 `ellipsed_api_key` 改为纯计算字段，不存储在数据库中，从 schema 中移除该列。

---

## 4. 数据库安全

### 4.1 ✅ 良好：全面使用参数化查询

所有数据库操作均使用 `sqlx` 的参数化查询（`$1`, `$2` 占位符），有效防止 SQL 注入攻击。

---

### 4.2 🟡 中危：动态 SQL 拼接存在潜在风险

**文件：** `crates/llmpool/src/db/openai.rs`

```rust
let mut sql = String::from("SELECT COUNT(*) FROM llm_models m ...");
if filter.upstream_id.is_some() {
    param_idx += 1;
    sql.push_str(&format!(" AND m.upstream_id = ${}", param_idx));
}
```

`count_models_filtered` 和 `list_models_filtered_paginated` 函数使用字符串拼接构建动态 SQL。虽然过滤值本身通过参数绑定传入（安全），但参数索引（`$1`, `$2`...）是通过字符串格式化生成的。若参数计数逻辑出现 bug，可能导致参数错位，引发数据泄露或逻辑错误。

**建议：** 使用 `sqlx::QueryBuilder` 替代手动字符串拼接，它提供更安全的动态查询构建方式。

---

### 4.3 🟢 低危：`session_events` 表使用 UNLOGGED 模式

**文件：** `crates/llmpool/migrations/20260319000001_initial_schema.sql`

```sql
CREATE UNLOGGED TABLE IF NOT EXISTS session_events (...)
```

`UNLOGGED` 表在 PostgreSQL 崩溃后不会自动恢复，可能导致计费数据丢失。虽然这是性能优化，但对于涉及资金的审计日志，数据完整性更为重要。

**建议：** 评估是否将 `session_events` 改为普通表，或至少确保有其他机制（如异步备份）保证数据不丢失。

---

### 4.4 🟢 低危：数据库迁移文件中的索引名称错误

**文件：** `crates/llmpool/migrations/20260319000001_initial_schema.sql`

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_credentials_apikey ON api_credential (apikey);
CREATE INDEX IF NOT EXISTS idx_api_credentials_account_id ON api_credential (account_id);
```

索引定义中表名为 `api_credential`（单数），但实际表名为 `api_credentials`（复数）。这会导致迁移执行失败，索引无法创建，进而影响 API Key 查询性能（全表扫描）。

**建议：** 修正表名为 `api_credentials`。

---

### 4.5 🟢 低危：`funds` 表缺少行级锁保护

**文件：** `crates/llmpool/src/defer/tasks/balance_change.rs`

余额变更使用了事务和 `FOR UPDATE` 锁（`find_balance_change_by_id_with_tx`），但 `funds` 表的更新操作是否也在同一事务中加锁需要确认。若并发处理多个余额变更任务，可能存在竞态条件。

**建议：** 确认 `apply_balance_change_with_tx` 在更新 `funds` 表时使用 `SELECT ... FOR UPDATE` 锁定对应行。

---

## 5. API 安全

### 5.1 🔴 高危：Passthrough 代理无用户认证

**文件：** `crates/llmpool/src/views/passthrough.rs`

```rust
pub fn get_router(pool: DbPool) -> Router {
    Router::new()
        .route("/tag/{tag}/{*rest}", any(passthrough_by_tag_handler))
        .route("/{upstream_id}/{*rest}", any(passthrough_by_upstream_id_handler))
        .route_layer(middleware::from_fn(admin_auth::auth_jwt))  // 仅管理员 JWT
```

Passthrough 代理路由（`/passthrough/*`）仅使用管理员 JWT 认证，普通用户无法使用。但更重要的是，该路由会将**任意 HTTP 请求**（包括 DELETE、PUT 等）透传到上游 API，且会自动注入上游的 API Key。

这意味着拥有管理员 JWT 的用户可以：
- 向上游发送任意请求（包括删除模型、修改配置等破坏性操作）
- 绕过所有业务逻辑（余额检查、审计日志等）

**建议：**
- 明确限制 Passthrough 允许的 HTTP 方法（如仅允许 GET、POST）
- 记录所有 Passthrough 请求的审计日志
- 评估是否需要此功能，若非必要可考虑移除

---

### 5.2 🟡 中危：CORS 配置过于宽松

**文件：** `crates/llmpool/src/server.rs`

```rust
.nest("/openai/v1", openai_router.layer(CorsLayer::very_permissive()))
```

`CorsLayer::very_permissive()` 允许所有来源（`*`）、所有方法、所有请求头的跨域请求。这意味着任何网站都可以通过浏览器向 OpenAI 代理端发起请求，可能导致 CSRF 攻击或 API Key 被恶意网站滥用。

**建议：**
- 配置明确的允许来源白名单
- 若 API 仅供服务端调用，可完全禁用 CORS

---

### 5.3 🟡 中危：错误信息可能泄露内部细节

**文件：** `crates/llmpool/src/views/admin_rest_api.rs`

```rust
error_response(
    StatusCode::BAD_GATEWAY,
    "upstream_error",
    &format!("Failed to save upstream: {}", e),  // 直接暴露错误详情
)
```

部分错误响应直接将内部错误信息（包括可能的数据库错误、网络错误详情）返回给客户端，可能泄露系统内部结构信息。

**建议：**
- 对外返回通用错误消息，将详细错误记录到日志
- 区分用户错误（4xx）和系统错误（5xx）的错误信息详细程度

---

### 5.4 🟢 低危：请求体大小无限制

**文件：** `crates/llmpool/src/server.rs`

Axum 默认对请求体大小有限制（2MB），但代码中未显式配置。对于 OpenAI 文件上传接口（`/openai/v1/files`），大文件上传可能消耗大量内存。

**建议：** 显式配置请求体大小限制，对文件上传接口单独设置合理的上限。

---

## 6. Redis 缓存安全

### 6.1 🟡 中危：Redis 无认证配置

**文件：** `docker/docker-compose.yml`、`llmpool.toml.example`

```yaml
redis:
  image: redis:7-bookworm
  ports:
    - "6379:6379"  # 暴露到宿主机
```

Docker Compose 中 Redis 未配置密码认证，且将 6379 端口暴露到宿主机。Redis 中存储了 API Key 信息、账户余额等敏感数据，未授权访问可能导致：
- 读取/篡改 API Key 缓存
- 读取/篡改账户余额缓存
- 向任务队列注入恶意任务

**建议：**
- 为 Redis 配置密码认证（`requirepass`）
- 在生产环境中不将 Redis 端口暴露到公网
- 在 Redis URL 中包含密码：`redis://:password@host:6379`

---

### 6.2 🟢 低危：缓存键无命名空间隔离

**文件：** `crates/llmpool/src/redis_utils/caches/apikey.rs`

```rust
fn apikey_cache_key(apikey: &str) -> String {
    format!("apikey:info:{}", apikey)
}
```

缓存键直接使用 API Key 字符串作为后缀。若 Redis 实例被多个服务共享，可能存在键名冲突风险。

**建议：** 添加应用级命名空间前缀，如 `llmpool:apikey:info:{apikey}`。

---

## 7. 代理与上游请求安全

### 7.1 🟡 中危：代理 URL 未验证，存在 SSRF 风险

**文件：** `crates/llmpool/src/views/passthrough.rs`、`crates/llmpool/src/views/openai_proxy/helpers.rs`

上游的 `api_base` URL 和 `proxies` 列表在存储时未经过严格验证。管理员可以配置指向内网地址的 `api_base`（如 `http://192.168.1.1/`），服务器会向该地址发起请求，形成服务端请求伪造（SSRF）攻击面。

```rust
let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
```

代理 URL 仅做了基本格式验证（`expect`），未过滤内网地址。

**建议：**
- 对 `api_base` 和 `proxies` 进行 URL 验证，拒绝私有 IP 地址范围（RFC 1918）
- 或在网络层面限制服务器的出站连接范围

---

### 7.2 🟢 低危：上游请求无超时配置

**文件：** `crates/llmpool/src/views/openai_proxy/helpers.rs`

```rust
let http_client = reqwest::Client::builder()
    .proxy(proxy)
    .build()
    .expect("Failed to build reqwest client with proxy");
```

构建 `reqwest::Client` 时未设置连接超时和请求超时。若上游服务无响应，请求将无限期挂起，可能耗尽服务器连接池资源。

**建议：** 配置合理的超时时间：
```rust
reqwest::Client::builder()
    .timeout(Duration::from_secs(300))
    .connect_timeout(Duration::from_secs(10))
    .build()
```

---

## 8. 资金与余额安全

### 8.1 ✅ 良好：余额变更使用事务和幂等性保护

`balance_changes` 表有 `unique_request_id` 唯一约束，防止重复扣费。余额变更通过异步任务队列处理，使用数据库事务和 `FOR UPDATE` 锁防止并发问题。

---

### 8.2 🟡 中危：余额检查与请求处理之间存在 TOCTOU 竞态

**文件：** `crates/llmpool/src/views/openai_proxy/helpers.rs`

```rust
pub async fn check_fund_balance(state: &AppState, account_id: i32) -> Result<(), Response> {
    // 检查余额
    if fund.cash.clone() <= BigDecimal::from(0) {
        return Err(...);  // 余额不足
    }
    Ok(())  // 通过检查，继续处理请求
}
```

余额检查（`check_fund_balance`）和实际扣费（异步任务）之间存在时间窗口。在高并发场景下，账户余额可能在检查通过后、扣费完成前被其他请求耗尽，导致账户余额变为负数（超支）。

**建议：**
- 实现预扣费机制：请求开始时预扣估算金额，完成后结算实际金额
- 或在扣费时检查余额是否足够，不足时标记账户为欠费状态

---

## 9. 配置与部署安全

### 9.1 🔴 高危：Docker Compose 使用弱密码

**文件：** `docker/docker-compose.yml`

```yaml
POSTGRES_USER: llmpool
POSTGRES_PASSWORD: llmpool  # 弱密码
```

Docker Compose 中 PostgreSQL 使用了与用户名相同的弱密码 `llmpool`，且数据库端口（5432）暴露到宿主机。

**建议：**
- 使用强随机密码，通过 `.env` 文件或 Docker Secrets 注入
- 生产环境中不将数据库端口暴露到公网
- 提供 `.env.example` 文件，并在 `.gitignore` 中排除 `.env`

---

### 9.2 🟢 低危：配置文件可能被意外提交到版本控制

**文件：** `.gitignore`

需确认 `llmpool.toml`（包含数据库密码、JWT Secret、加密密钥）已被加入 `.gitignore`。

**建议：** 确认 `.gitignore` 包含 `llmpool.toml` 和 `.env`。

---

## 10. 依赖安全

### 10.1 主要依赖版本

| 依赖 | 版本 | 说明 |
|------|------|------|
| `axum` | 0.8.8 | Web 框架，版本较新 |
| `sqlx` | 0.8 | 数据库驱动，版本较新 |
| `jsonwebtoken` | 9 | JWT 库，版本较新 |
| `aes-gcm` | 0.10 | 加密库，版本较新 |
| `reqwest` | 0.12 | HTTP 客户端，版本较新 |
| `async-openai` | 0.33.1 | OpenAI 客户端 |
| `apalis` | 1.0.0-rc.6 | 任务队列，RC 版本 |

### 10.2 🟡 中危：使用 Release Candidate 版本依赖

`apalis` 和 `apalis-redis` 使用了 `1.0.0-rc.6` 版本（Release Candidate），这是预发布版本，可能存在未修复的 bug 或安全问题，且 API 可能在正式发布时发生变化。

**建议：** 关注 `apalis` 正式版本发布，及时升级到稳定版本。

### 10.3 🟢 低危：未使用 `cargo audit` 进行依赖漏洞扫描

项目中未配置自动化依赖漏洞扫描（`cargo audit`）。

**建议：**
- 安装并运行 `cargo audit`：`cargo install cargo-audit && cargo audit`
- 在 CI/CD 流程中集成依赖漏洞扫描

---

## 11. 日志与可观测性安全

### 11.1 🟡 中危：使用 `eprintln!` 而非结构化日志

**文件：** `crates/llmpool/src/views/openai_proxy/chat_completions.rs`

```rust
eprintln!("No client for model {model_name}");
eprintln!("Chat completion failed after retry: {:?}", e);
eprintln!("Stream item error: {:?}", e);
```

部分错误使用 `eprintln!` 输出到 stderr，而非使用 `tracing` 框架的结构化日志。这导致：
- 日志无法被集中收集和分析
- 错误信息可能包含敏感数据（如模型名称、错误详情）
- 无法通过日志级别控制输出

**建议：** 将所有 `eprintln!` 替换为 `tracing::error!` 或 `tracing::warn!`。

---

### 11.2 🟢 低危：请求日志可能记录敏感信息

**文件：** `crates/llmpool/src/server.rs`

```rust
.layer(TraceLayer::new_for_http())
```

`TraceLayer` 会记录所有 HTTP 请求信息，包括请求头（可能包含 `Authorization: Bearer <apikey>`）。

**建议：** 配置 `TraceLayer` 过滤敏感请求头，避免将 API Key 记录到日志中。

---

## 12. 问题汇总与优先级

| 编号 | 风险等级 | 问题描述 | 文件位置 |
|------|---------|---------|---------|
| 2.1 | 🔴 高危 | JWT 令牌未验证过期时间 | `middlewares/admin_auth.rs` |
| 2.2 | 🔴 高危 | OpenAI 代理端缺少速率限制 | `views/openai_proxy/mod.rs` |
| 3.1 | 🔴 高危 | 加密密钥为可选，默认明文存储 API Key | `crypto.rs`, `config.rs` |
| 5.1 | 🔴 高危 | Passthrough 代理可绕过业务逻辑 | `views/passthrough.rs` |
| 9.1 | 🔴 高危 | Docker Compose 使用弱密码且暴露端口 | `docker/docker-compose.yml` |
| 2.3 | 🟡 中危 | API Key 缓存命中后仍查询数据库 | `views/openai_proxy/helpers.rs` |
| 2.4 | 🟡 中危 | 管理端 API 与用户端共用端口 | `server.rs` |
| 3.2 | 🟡 中危 | JWT Secret 强度无校验 | `config.rs` |
| 3.3 | 🟡 中危 | 数据库密码明文存储在配置文件 | `llmpool.toml.example` |
| 4.2 | 🟡 中危 | 动态 SQL 拼接存在潜在风险 | `db/openai.rs` |
| 5.2 | 🟡 中危 | CORS 配置过于宽松 | `server.rs` |
| 5.3 | 🟡 中危 | 错误信息可能泄露内部细节 | `views/admin_rest_api.rs` |
| 6.1 | 🟡 中危 | Redis 无认证配置且端口暴露 | `docker-compose.yml` |
| 7.1 | 🟡 中危 | 上游 URL 未验证，存在 SSRF 风险 | `views/passthrough.rs` |
| 8.2 | 🟡 中危 | 余额检查与扣费之间存在竞态条件 | `views/openai_proxy/helpers.rs` |
| 10.2 | 🟡 中危 | 使用 RC 版本依赖（apalis） | `Cargo.toml` |
| 11.1 | 🟡 中危 | 使用 `eprintln!` 而非结构化日志 | `views/openai_proxy/chat_completions.rs` |
| 2.5 | 🟢 低危 | API Key 使用时间有序 UUID | `db/api.rs` |
| 3.4 | 🟢 低危 | `ellipsed_api_key` 冗余存储 | `migrations/*.sql` |
| 4.3 | 🟢 低危 | `session_events` 使用 UNLOGGED 表 | `migrations/*.sql` |
| 4.4 | 🟢 低危 | 数据库迁移中索引表名错误 | `migrations/*.sql` |
| 4.5 | 🟢 低危 | `funds` 表行级锁保护需确认 | `defer/tasks/balance_change.rs` |
| 5.4 | 🟢 低危 | 请求体大小无显式限制 | `server.rs` |
| 6.2 | 🟢 低危 | Redis 缓存键无命名空间隔离 | `redis_utils/caches/` |
| 7.2 | 🟢 低危 | 上游请求无超时配置 | `views/openai_proxy/helpers.rs` |
| 9.2 | 🟢 低危 | 配置文件可能被提交到版本控制 | `.gitignore` |
| 10.3 | 🟢 低危 | 未使用 `cargo audit` 扫描依赖漏洞 | `Cargo.toml` |
| 11.2 | 🟢 低危 | 请求日志可能记录敏感信息 | `server.rs` |

---

## 13. 修复建议

### 立即修复（高危）

1. **启用 JWT 过期验证**
   ```rust
   // admin_auth.rs
   let mut validation = Validation::default();
   // 移除以下两行：
   // validation.required_spec_claims.remove("exp");
   // validation.validate_exp = false;
   ```

2. **强制要求加密密钥**
   ```rust
   // config.rs 或 main.rs 启动时
   if cfg.security.encryption_key.is_empty() {
       panic!("security.encryption_key must be configured in production");
   }
   ```

3. **添加速率限制**
   ```toml
   # Cargo.toml
   tower = { version = "0.5", features = ["limit"] }
   ```

4. **修复 Docker Compose 弱密码**
   ```yaml
   # docker-compose.yml
   environment:
     POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}  # 从 .env 读取
   ```

### 短期修复（中危，建议 1-2 周内完成）

5. **修复数据库迁移中的表名错误**（`api_credential` → `api_credentials`）

6. **配置 Redis 认证**，并在生产环境中不暴露 Redis 端口

7. **收紧 CORS 配置**，配置明确的允许来源

8. **添加 `api_base` URL 验证**，防止 SSRF

9. **为上游请求配置超时**

10. **将 `eprintln!` 替换为 `tracing::error!`**

### 长期改进（低危，建议纳入迭代计划）

11. 集成 `cargo audit` 到 CI/CD 流程

12. 实现预扣费机制解决余额竞态问题

13. 将管理端 API 与用户端 API 分离到不同端口

14. 配置 `TraceLayer` 过滤敏感请求头

15. 评估 `session_events` 表是否需要改为普通表

---

*本报告基于代码静态分析，建议在修复后进行渗透测试以验证修复效果。*
