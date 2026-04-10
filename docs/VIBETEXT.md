

========
新建一个数据库 SessionEvent, 具有字段session_id, user_id, model_id, event_data

实现 SessionTracer 的两个方法
* new(user_id, model_id), session_id 根据uuidv7生成
* add(data: SessionEventData), 生成一条 SessionEvent 记录，并插入到数据库中

在chat_completions，generate_images 和create_embedding函数中, 将现有的ChatSessionLog 方法替换成 SessionTracer的new和add方法

删除ChatSessionLog数据库表和相关结构, migration可以直接修改和删除，不需要新建。

========
创建一个数据库模型UserBalance, 具有字段user_id, balance, debt, balance 和debt 都是decimal类型
创建一个数据库模型BalanceChange, 具有字段user_id, content, content 是json字段，格式如下enum
```rust
enum BalanceChangeContent {
    SpendToken(amount: decimal, event_id: uuid, price: decimal),
    Deposit(amount: decimal),
    Withdraw(amount: decimal),
}
```

========
添加admin api 功能，使用 jsonrpc 实现，文件写在views/admin_rpc.py 中, url_prefix 为 /admin/rpc
使用jwt 认证, jwt secret 在配置文件llmpool.toml中定义.
添加jsonrpc method: getBalance, 获得UserBalance信息, 参数: user_id, 返回值: {"balance": decimal, "debt": decimal}
添加jsonrpc method: createUser, 创建用户, 参数: username, 返回值： user_id
添加jsonrpc method: createApiKey, 创建api key, 参数: user_id, 返回值: api_key

========
实现一个 clap admin jwt-token 命令，用于生成jwt token, 可选指定expire时间

========
LLMModel 增加两个字段 input_token_price, output_token_price, 用于记录输入和输出的token价格, 类型都是decimal, 默认为0.000001
将 SessionTracer 的 add 方法中的SessionEventData 提取出event_data的不同payload中，提取出里面的usage(如果有), 并根据usage 计算输入和输出的token数量，从LLMModel的input_token_price 和output_token_price字段中获取价格, 共同生成SpendToken对象以及存在BalanceChange中。

========
写一个数据库方法 apply_balance_change，并用在session_tracer 的 add 方法中，用于更新UserBalance信息
当BalanceChange.content 为SpendToken时, UserBalance.balance -= SpendToken.input_spend_amount + SpendToken.output_spend_amount. UserBalance.balance < 0 时, UserBalance.debt += 剩下的金额， UserBalance.balance清零.
当BalanceChange.content 为Deposit时, UserBalance.balance += Deposit.amount.
当BalanceChange.content 为Withdraw时, UserBalance.balance -= Withdraw.amount. UserBalance.balance < 0 时, UserBalance.debt += 剩下的金额， UserBalance.balance清零.

========
实现以下功能:
在chat_completions, generate_images 和create_embedding函数中, 如果选定client 后访问发生异常或者错误，则重新选一个client 重试一次，如果仍然失败，则返回错误。

========
UserBalance 增加字段 credit, 默认为0, SpendToken 时优先扣除credit, 如果不够，则从cash中扣除, 如果仍然不够，则加入debt.
Deposit和Withdraw 都不更改credit。
直接修改migrations 文件，不用新建migration文件

========
引入redis 支持, 使用crate redis-rs. redis_url 在config中定义. 使用时优先从环境变量REDIS_URL中获取, 如果不存在，则从config中获取.
使用 apalis以及redisstore 实现异步任务队列。队列任务定义在文件 defer/tasks.rs 中。
实现一个异步任务，handle_event(event: EventEntry); 实现 sesssion_tracer 的 add 的逻辑。
实现clap 子命令 llmpool defer worker, 用于处理异步队列任务。

========
在docker 目录下生成一个Dockerfile 以及用于开发的docker-compose.yml 文件，用于启动redis, postgres, llmpool, llmpool-defer

========
实现一个异步任务 handle_balance_change(balance_change_id: i64); 将 apply_balance_change 方法的逻辑实现在异步任务中。

========
BalanceChange 对象增加一个字段is_applied, 缺省为False, 当apply完毕后设置成true.   initial_schema和migration文件可以直接修改，不需要新建。handle_balance_change的执行要在一个数据库事务中执行。

========
使用axum 实现一个restful api, 用于管理llmpool的运行，验证采用jsonrpc同样的方法和secret, 先实现 GET /api/v1/upstreams 方法，返回当前运行中的upstream列表, 支持page参数。

========
再实现一个RESTful api, 用于管理用户 GET|POST /api/v1/users。

========
SessionEvent 增加一个session_index 字段，从OpenAIEventTask 中获取； initial_schema和migrations 文件可以修改，不用新增。

========
LLMUpstream 添加一个字段tags: Vec<String>, 用于记录该upstream的标签.

========
在文件 views/passthrough.rs 中，实现透传request 和 response 的逻辑。URL /passthrough/tag/:tag/:rest, 通过tag 找到对应的upstream list, 随机选择一个upstream, 使用reqwest, 透传request 和 response。url 重写为 /:rest.

========
实现 /passthrough/:upstream_id/:rest, 根据upstream_id 找到对应的upstream, 透传request 和 response。url 重写为 /:rest.

========
admin rest api 使用"x-admin-token" header作为验证， header的内容是jwt token，取代以前的auth bearer token. passthrough 也使用x-admin-token header作为验证。并更新API.md.

========
middleware auth_jwt 的逻辑抽出来单独放在 middlewares/admin_auth.rs 中，并修改passthrough.rs 和admin_rest.rs 中使用middleware.

========
给LLMUpstream 增加一个字段 proxies: Vec<String>, 用于记录该upstream的代理地址, 在建立passthrough 客户端和 openaiclient 时，如果proxies 不为空，则从中随机选择一个作为代理地址。openaiclient 可以用底层的 reqwest::Client::builder() 方法设置代理地址。

========
LLMUpstream 添加一个字段 status: String, 用于记录该upstream的运行状态, 可选值为 online, offline, maintenance, 默认值为online.
LLMUpstream 添加一个字段 description: String, 用于记录该upstream的描述信息, 缺省为空。
LLMModel 添加一个字段 description: String, 用于记录该model的描述信息, 缺省为空。
可以直接修改migrations 文件，不用新建migration文件。

增加 admin api: GET /api/v1/upstreams/:upstream_id, 根据upstream_id 获得upstream信息
增加 admin api: PUT /api/v1/upstreams/:upstream_id, 修改upstream信息，可修改字段为 name, tags, proxies, description,status

增加 admin api: GET /api/v1/models/:model_id, 根据model_id 获得model信息
增加 admin api: PUT /api/v1/models/:model_id, 修改model_id信息，可修改字段为 description

========
在crates llmpool-ctl 中实现一个命令行工具，用于通过调用admin api管理llmpool。通过环境变量 LLMPOOL_ADMIN_URL 设置admin api的url; LLMPOOL_ADMIN_TOKEN 设置admin api的token. 程序可以读取当前目录下.env文件，读取环境变量。 命令行工具应该实现以下功能:
1. llmpool-ctl upstream list 显示所有upstream信息
2. llmpool-ctl upstream test --api-key <api-key> --api-base <api-url> 测试一个upstream是否可用
3. llmpool-ctl upstream add --name <name> --api-key <api-key> --api-base <api-url> [--description <description>] [--tags <tags>] [--proxies <proxies>] 创建一个upstream
4. llmpool-ctl upstream update --upstream-id <upstream-id> [--name <name>] [--description <description>] [--tags <tags>] [--proxies <proxies>] [--status <status>] 更新一个upstream
5. llmpool-ctl model list 显示所有model信息
6. llmpool-ctl model update --model-id <model-id> [--description <description>]
7. llmpool-ctl user list 显示所有用户信息

========
llmpool-ctl 添加以下命令
1. llmpool-ctl user add <username>
1. llmpool-ctl fund show --user <username_or_id>, 查看用户余额
1. llmpool-ctl fund deposit --user <username_or_id> --amount <amount> --request-id <request-id>, 充值
1. llmpool-ctl fund withdraw --user <username_or_id> --amount <amount> --request-id <request-id>, 提现
1. llmpool-ctl fund credit --user <username_or_id> --amount <amount> --request-id <request-id>, 增加信用

<username_or_id> 如果时用户名，则通过/users_by_name 查找用户id

=========
AccessKey增加一个字段 label，用于记录该key的用途。 直接修改migraion 文件，不用新建migration文件。

llmpool-ctl 添加以下命令
1. llmpool-ctl apikey list --user <username_or_id>，显示用户的apikey列表
1. llmpool-ctl apikey add --user <username_or_id> --label <label>，新增一个apikey
1. llmpool-ctl user show --user <username_or_id>, 显示用户信息

=========
AccessKey 修改为 LLMAPIKey, 直接修改migraion 文件，不用新建migration文件。

ADMIN api 增加如下命令
1. PUT /api/v1/users/:user_id, 修改用户信息，可修改字段为 username, is_active

llmpool-ctl 添加以下命令
1. llmpool-ctl user update --user <username_or_id> [--username <username>] [--is-active <is-active>]

=========
jwt token 需要添加realm=api, 在auth的时候要检查realm 是否等于"api"

=========
实现 upstream tags操作的 admin api
1. GET /api/v1/upstreams/:upstream_id/tags, 获得upstream的tags列表
1. POST /api/v1/upstreams/:upstream_id/tags, 添加一个tag
1. DELETE /api/v1/upstreams/:upstream_id/tags/:tag, 删除一个tag

实现 admin api GET /api/v1/upstream_by_name/:name, 通过upstream name 获得upstream 信息

llmpool-ctl 添加以下命令
1. llmpool-ctl upstream listtags --upstream <upstream_name_or_id>, 显示upstream的tags列表
1. llmpool-ctl upstream addtag --upstream <upstream_name_or_id> --tag <tag>, 添加一个tag
1. llmpool-ctl upstream deltag --upstream <upstream_name_or_id> --tag <tag>, 删除一个tag

<upstream_name_or_id> 如果是名字，则通过/upstream_by_name 查找upstream_id, 命令 llmpool-ctl upstream update 也修改成这种参数

=========
llmpool-ctl root 程序支持命令行参数 --format <format>, <format> 可以是"", 和"json", 如果是"json", 则输出json格式的响应。

=========
将一级子命令分拆到不同文件中，比如 upstream xxx 命令可以放在 cmds/upstream.rs 中, ...

=========
model 的admin api信息中应该有price, 也可以修改price 字段

=========
db model User 全局换成Consumer

=========
SessionEvent 增加一个字段 api_key_id，用于记录该session使用的api_key_id. task_local 对象LLM_API_KEY中获得， 可以在session_tracer.rs中加入SessionTracer结构中。
增加 admin api: GET /api/v1/sessionevents/, 支持session=<session_id>参数 获得SessionEvent列表
llmpool-ctl sessionevents list [--session <session_id>] 显示SessionEvent列表

=========
给数据库相关操作生成一些test cases

=========
SessionEvent 增加以下字段: input_token_price, input_tokens, output_token_price, output_tokens。admin api也的response也相应增加。直接修改migraions 文件，不用新建migration文件。

=========
admin API /api/v1/sessionevents/ 的参数不要page size, page, 改成 start=<event_id>, count=<count>, 返回值改成
```json
{
    "data": ...
    "next_id": <next_id>,
    "has_more": true/false
}
```

=========
设计一个基于redis的按小时累积的usage counter, key 是 "tokenusage:input:<model.id>:<hour>" 和 "tokenusage:output:<model.id>:<hour>"。 在handle_openai_event 方法中获得usage以后，按照对应的key采用redis 的incr方法累加usage。

=========
redis 使用bb8_redis，建立连接池，在worker里和数据库连接池DbPool一样维护和传入，在increment_token_usage 方法中，使用redis_pool.get() 获得redis连接，使用redis_conn.incr() 累加usage。

将increment_token_usage 方法转移到 src/redis_utils/counters.rs 中

将select_model_clients 中随机选取的逻辑，换成使用redis 获取tokenusage.output 的值(如没有相关key value, 则按默认为0算)，取其中最小的count个用于随后的请求。并将redis相关的逻辑也加入到redis_utils/counters.rs 中。

=========
DB model Consumer 的名字换成 Account, admin api 中相应修改, llmpool-ctl 命令中相应修改, 相关文档也需要修改。 migraions 文件可以直接修改，不需要新建。

=========
添加一个admin api: GET /api/v1/apikeys/:<apikey>, 获得对应apikey的信息
添加一个admin api: DELETE /api/v1/apikeys/:<apikey>, 将对应apikey删除，在数据库不真实删除，而是将is_active字段设置为false
在redis_utils/cache.rs 中添加如下方法
* get_apikey_info(apikey: &str) -> Result<ApiKeyInfo>, 从cache中获得apikey的信息。
* set_apikey_info(apikey: &str, info: ApiKeyInfo) -> Result<()> 设置apikey的信息到cache中。
* delete_apikey(apikey: &str) -> Result<()> 删除apikey cache缓存。
在openai_proxy.rs 中使用get_apikey_info, 在admin_rest_api.rs 中使用set_apikey_info.

=========
将redis_utils/cache.rs 移动到 redis_utils/caches/apikey.rs中。

在redis_utils/caches/account.rs 中添加如下方法:
* get_account_info(account_id: &str) -> Result<AccountInfo>, 从cache中获得account的信息。
* delete_account(account_id: &str) -> Result<()>, 删除cache 缓存
在openai_proxy.rs 中使用get_apikey_info, 在admin_rest_api.rs update_account方法中, 使用delete_account

=========
在redis_utils/caches/fund.rs 中添加如下方法:
* get_fund_info(fund_id: &str) -> Result<FundInfo>, 从cache中获得fund的信息。
* set_fund_info(fund_id: &str, info: FundInfo) -> Result<()>
在apply_balance_change 方法中, 使用set_fund_info
在openai_proxy.rs 中, 添加一个task_local 变量 FUND: Fund, 在获得ACCOUNT 后设置此变量.
在需要花费token的function，如chat_completions, create_embedding, generate_images中，检查 FUND.cash + FUND.credit > 0, 否则拒绝请求并报错账户余额不够.

取消task_local变量 FUND, 在chat_completions, create_embedding, generate_images中，中调用一个 check_fund_balance(account_id) 的方法，在方法中实现 get_fund_info, 如果没拿到就从数据库中拿到， 检查 FUND.cash + FUND.credit > 0, 否则拒绝请求并报错账户余额不够.

=========
LLMUpstream 增加一个字段： provider, 默认为 "openai", 可以是 "openai", "azure", "cohere", "anthropic", "vllm", "ollama", 在admin api中也显示此参数，在创建upstream 时，可以带上这个参数。 可直接修改migrations 文件，不用新建migration文件。

=========
实现admin api: GET /api/v1/session-events/:event_id, 获得session event 的详细信息, 并更新llmpool-ctl 命令, 和api-schema.json|yaml 文件。 更新docs/api.md 文件。

=========
在 openai_proxy.rs 中，实现对 /v1/files 和/v1/batches, 以及 /v1/batches/:batch_id 的代理

=========
将openai_proxy.rs 中 /v1/files 的处理逻辑，移动到 src/openai_proxy/files.rs 中; 处理 batches 的逻辑，移动到 src/openai_proxy/batches.rs 中；处理 chat_completions 的逻辑，移动到 src/openai_proxy/chat_completions.rs 中; 处理 speech 的逻辑，移动到 src/openai_proxy/speech.rs 中； images 的逻辑，移动到 src/openai_proxy/images.rs 中, 其他逻辑也依次类推

=========
实现一个DB model: FileMeta, 表结构如下:
* id: i64,
* file_id: string, uuidv7 格式 的唯一值
* original_file_id: string, upstream 生成的file_id
* purpose: string,
* deleted: bool, 标记是否删除
直接修改过migrations 文件，不用新建migration文件。

创建文件create_file_handler中，生成一个file_id, 将CreateFileResponse 的file_id 设置为original_file_id, 将file_id 存储到file_meta 表中。 
retrieve_file_handler, delete_file_handler, file_content_handler 中，根据file_id获得original_file_id, 返回response 中再将file_id换回来。
数据库操作代码写在 db/files.rs 中

file_meta 表中，添加一个字段: upstream_id, 创建文件时，将upstream.id 存储到file_meta 表中。
retrieve_file_handler, delete_file_handler, file_content_handler， 根据file_meta的upstream_id 获得upstream, 不再使用select_first_upstream.

=========
创建一个DB model: BatchMeta, 表结构如下:
* id: i64,
* batch_id: string, uuidv7
* original_batch_id: string, upstream 生成的batch_id
* upstream_id: i64,
* status: string, 默认为"pending", 可选值: "pending", "processing", "completed", "canceled"
* created_at: timestamp,

创建create_batch_handler 中，根据input_file_id, 从file_meta 表中获取upstream_id, 将请求转发到对应的upstream中。生成一个batch_id,  生成一个BatchMeta 记录，将原batch_id作为original_batch_id, 以及upstream_id 存储到BatchMeta 表中。

batch_cancel_handler中，根据batch_id, 从BatchMeta 表中获取upstream_id, 不再使用select_first_upstream. 根据batch_id获得original_batch_id, 返回response 中再将batch_id换回来。

直接修改过migrations 文件，不用新建migration文件。

=========
LLMUpstream 添加一个字段: ellipsed_api_key, 存储 api_key 的前 6 个字符 + "..." + 最后 6 个字符, 直接修改过migrations 文件，不用新建migration文件。

LLMEndpoint 的 api_key 字段改名为 encrypted_api_key, 直接修改过migrations 文件，不用新建migration文件。

=========
batch处理的结构 Batch 中有个output_file_id对象，使用uuidv7生成一个新的file_id，将原来的output_file_id编程original_file_id中，并存在file_meta 表中。用新的file_id 填充到返回的Batch结构的output_file_id字段中。 

==========
LLMModel 增加两个字段 batch_input_token_price 和batch_output_token_price, 缺省价格和原来的价格一致。直接修改migrations文件，不需要新建migration文件。
UsageInfo 增加一个is_batch字段，默认为false。生成SpendToken的时候，如果usage.is_batch 则价格使用 batch_output_token_price和batch_input_token_price

==========
实现admin API,  /api/v1/models/path/{upstream_name}/{*model_name} 根据path("{upstream_name}/{model_name}") 显示model的详情

实现 admin API, POST /api/v1/models-tests, 参数 {"model_ids": Vec[i32]}, 对model_ids 指定的model 测试其features 并更新到LLMModel表，不更改其它字段。

=========
实现命令 llmpool-ctl model detect --model <PATH_OR_ID>,  对指定的model调用/api/v1/models-test 进行特性测试，并返回测试后model 对象详情. PATH_OR_ID 可以是model path 或者 model id, 如果是path, 则会调用/api/v1/models/path/{upstream_name}/{*model_name} 获得model_id 再调用/api/v1/models-tests。

实现命令 llmpool-ctl model show --model <PATH_OR_ID>, 显示指定model的详情，model详情字段增加 is_active, batch_input_token_price, batch_output_token_price. 可修改字段也增加这三个。如API没有提供，就修改api响应数据以及api-schema.json。

========== 
将 Fund.cash 和Fund.debt 合并成一个字段 Fund.balance. 删除 Fund.cash 和Fund.debt. balance可以为负数，此时为debt.

==========
使用 axum_rate_limiter 实现基于redis 的ratelimit 功能

==========
当chat_completions_with_client 遇到 OpenAIError::Reqwest 时，返回, 可以认为网络出现错误，将对应的upstream.status 标记为"offline". chat_completions_with_client, create_speech_with_client, create_embeddings_with_client, 都这样处理。
写一个apalis cronjob, 每5分钟并发检查一次所有的status 为offline的upstream，访问upstream的/models 接口，如果返回200，则将upstream.status 改为"online".

==========
使用anthropic sdk, 实现类似于openai api代理的anthropic api代理，先实现第一个方法 /v1/messages POST.
anthropic api 的认证方法使用 auth_anthropic_api, 和auth_openai_api 的区别在于使用x-api-key header认证，而不是authorization header.

==========
根据docs/anthropic-api.json 使用reqwest生成anthropic api的client端代码, 使用header x-api-key 验证, 存在 anthropic_proxy/anthropic_api.rs 中.

==========
仿照OpenAI的EventData, 实现 AnthropicEventData, 对应的SessionTracer, 以及将SessionTracer.add 加入到代理的方法中。直接修改migrations文件，不需要新建migration文件。 还有对应的task handle_anthropic_event.

==========
LLMModel增加一个字段 has_messages: bool(default false), 用来判断是否有authropic 的 /v1/messages 接口, detect has_messages代码写在 anthropic/features.rs 中
在helpers/features.rs 增加一个detect_and_save_features方法, 分别调用 openai和anthropic的detect_and_save_features方法。
在helpers/features.rs 增加一个detect_and_update_model_features方法, 分别调用 openai和anthropic的detect_and_update_model_features方法。

==========
CapacityOption增加一个字段, has_messages, find_models_by_name_and_capacity不再依赖provider 去寻找models, authropic中使用has_messages查找models. 

==========
设计如下的订阅接口和数据库Model
* SubscriptionPlan: 字段有 id, status, description, input_token_limit, output_token_limit, money_limit, start_at, end_at,created_at, updated_at, sort_order: integer
  status 可以选的值为 "created", "started", "active", "canceled", "expired"
* Subscription: 字段有 id, account_id, plan_id, status, used_input_tokens, used_output_tokens, used_money, created_at, updated_at
  status 可以选的为 "deducted", "active", "filled"

直接修改migrations文件，不需要新建migration文件。
写一个方法 get_current_subscription(account_id): Optional[Subscription]: 找到按照sort_order desc排序的第一个用户plan的订阅，如果已经过期或者还没有开始或者stauts != "active"，则返回None

创建admin API:
1. GET /api/v1/subscription-plans/
1. GET /api/v1/subscription-plans/:plan_id
1. POST /api/v1/subscription-plans/
1. PUT /api/v1/subscription-plans/:plan_id
1. DELETE /api/v1/subscription-plans/:plan_id, cancel plan
1. GET /api/v1/subscriptions/, 有filter参数 account_id, status
1. GET /api/v1/subscriptions/:subscription_id
1. POST /api/v1/subscriptions/,  参数 {account_id, plan_id}
1. PUT /api/v1/subscriptions/:subscription_id, 参数 {status}
1. DELETE /api/v1/subscriptions/:subscription_id, cancel subscription

更新 api-admin.json 和 llmpool-ctl 命令

===========
修改database model SubscriptionPlan: status choices 为active, deactive; 删除字段 input_token_limit, output_token_limit, start_at, end_at. 增加字段  total_token_limit, time_span: integer.
修改database model Subscription: status choices 为active, deactive; 删除字段 used_input_tokens, used_input_tokens, 添加字段:  start_at, end_at, used_total_tokens, total_token_limit, sort_order.
修改migrations文件即可，不需要新建migration文件
修改api-schema.json 以及llmpool-ctl 命令

重写 get_current_scubscription(pool, account_id, consumed_tokens), 选择条件包括 start_at <= now < end_at and status = 'active' and total_token_limit >= used_total_tokens + consumed_tokens

在apply_balance_change_with_tx中，如果遇到SpendToken, 则优先get_current_subscription_with_tx(tx, account_id, spend.input_tokens + spend.output_tokens) 找到subscription, 如果找到，则更新subscription将subscription.used_total_tokens += spend.input_tokens + spend.output_tokens. subscription.used_money += spend.input_spend_amount + spend.output_spend_amount.

==========
BalanceChange 增加字段subscription_id, 记录apply的对应subscription id. 修改migrations文件即可，不需要新建migration文件
mark_balance_change_applied_with_tx 增加参数subscription_id

==========
仿照 /openai/v1/chat/completions, 实现 openai responses api: POST /api/v1/responses 以及 GET /api/v1/responses/:id, 实现在openai_proxy/responses.rs

LLMModel 添加字段: has_responses_api, LLMUpstream中删除has_responses_api字段. openai/features.rs 中 ModelFeature 添加字段: has_responses_api, 在detect_model_features中添加判断.
CapacityOptions 添加字段: has_responses_api, 默认为false. 在responses.rs中中使用has_responses_api作为select_model_clients的过滤器。更新 api-schema.json 以及 llmpool-ctl 命令

修改migrations文件即可，不需要新建migration文件.


===========
LLMModel 删除字段: model_id, 新增两个字段 cname 和 fullname, fullname 就是原来的model_id, cname 是model_id中"/"后的那一部分，如果model_id中"/"不存在，则cname为model_id.
在find_models_by_name_and_capacity查询中，使用cname. 在client请求upstream时，使用fullname.

修改migrations文件即可，不需要新建migration文件.

============
创建一个DB model: ResponseMeta, 表结构如下:
* id: i64,
* response_id: string, uuidv7
* original_response_id: string, upstream 生成的response_id
* upstream_id: i64,
* deleted: bool,
* created_at: timestamp

create_response 中，在返回的Response对象中提取id, 记录在original_response_id, 生成新的ResponseMeta对象, 将uuidv7生成的response_id返回给客户端，并保存到DB中.

retrieve_response中，根据response_id, 从DB中获取ResponseMeta对象，找到对应的upstream并将original_response_id作为参数传递给upstream.

直接修改过migrations 文件，不用新建migration文件。

create_response() 中 CreateResponse.previous_response_id 也需要
通过ResponseMeta 换成original_response_id, 并在Response中换回来。
retrieve_response() 中，previouse_response_id需要通过ResponseMeta 换成original_response_id

在db/responses.rs 里写两个方法
* get_original_response_id(response_id: i32)
* get_response_id_from_original_response_id(original_response_id: i32)
用于original_response_id和response_id的的转换，并替换现有openai_proxy/respnses.rs中的方法

实现代理方法 DELETE /v1/responses/:id, 去对应的Upstream delete responses 以后 标记ResponseMeta.deleted = true