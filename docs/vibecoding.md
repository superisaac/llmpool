

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
OpenAIModel 增加两个字段 input_token_price, output_token_price, 用于记录输入和输出的token价格, 类型都是decimal, 默认为0.000001
将 SessionTracer 的 add 方法中的SessionEventData 提取出event_data的不同payload中，提取出里面的usage(如果有), 并根据usage 计算输入和输出的token数量，从OpenAIModel的input_token_price 和output_token_price字段中获取价格, 共同生成SpendToken对象以及存在BalanceChange中。

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
使用axum 实现一个restful api, 用于管理llmpool的运行，验证采用jsonrpc同样的方法和secret, 先实现 GET /api/v1/endpoints 方法，返回当前运行中的endpoint列表, 支持page参数。

========
再实现一个RESTful api, 用于管理用户 GET|POST /api/v1/users。

========
SessionEvent 增加一个session_index 字段，从OpenAIEventTask 中获取； initial_schema和migrations 文件可以修改，不用新增。

========
OpenAIEndpoint 添加一个字段tags: Vec<String>, 用于记录该endpoint的标签.

========
在文件 views/passthrough.rs 中，实现透传request 和 response 的逻辑。URL /passthrough/tag/:tag/:rest, 通过tag 找到对应的endpoint list, 随机选择一个endpoint, 使用reqwest, 透传request 和 response。url 重写为 /:rest.

========
实现 /passthrough/:endpoint_id/:rest, 根据endpoint_id 找到对应的endpoint, 透传request 和 response。url 重写为 /:rest.

========
admin rest api 使用"x-admin-token" header作为验证， header的内容是jwt token，取代以前的auth bearer token. passthrough 也使用x-admin-token header作为验证。并更新API.md.

========
middleware auth_jwt 的逻辑抽出来单独放在 middlewares/admin_auth.rs 中，并修改passthrough.rs 和admin_rest.rs 中使用middleware.

========
给OpenAIEndpoint 增加一个字段 proxies: Vec<String>, 用于记录该endpoint的代理地址, 在建立passthrough 客户端和 openaiclient 时，如果proxies 不为空，则从中随机选择一个作为代理地址。openaiclient 可以用底层的 reqwest::Client::builder() 方法设置代理地址。

========
OpenAIEndpoint 添加一个字段 status: String, 用于记录该endpoint的运行状态, 可选值为 online, offline, maintenance, 默认值为online.
OpenAIEndpoint 添加一个字段 description: String, 用于记录该endpoint的描述信息, 缺省为空。
OpenAIModel 添加一个字段 description: String, 用于记录该model的描述信息, 缺省为空。
可以直接修改migrations 文件，不用新建migration文件。

增加 admin api: GET /api/v1/endpoints/:endpoint_id, 根据endpoint_id 获得endpoint信息
增加 admin api: PUT /api/v1/endpoints/:endpoint_id, 修改endpoint信息，可修改字段为 name, tags, proxies, description,status

增加 admin api: GET /api/v1/models/:model_id, 根据model_id 获得model信息
增加 admin api: PUT /api/v1/models/:model_id, 修改model_id信息，可修改字段为 description

========
在crates llmpool-ctl 中实现一个命令行工具，用于通过调用admin api管理llmpool。通过环境变量 LLMPOOL_ADMIN_URL 设置admin api的url; LLMPOOL_ADMIN_TOKEN 设置admin api的token. 程序可以读取当前目录下.env文件，读取环境变量。 命令行工具应该实现以下功能:
1. llmpool-ctl endpoint list 显示所有endpoint信息
2. llmpool-ctl endpoint test --api-key <api-key> --api-base <api-url> 测试一个endpoint是否可用
3. llmpool-ctl endpoint add --name <name> --api-key <api-key> --api-base <api-url> [--description <description>] [--tags <tags>] [--proxies <proxies>] 创建一个endpoint
4. llmpool-ctl endpoint update --endpoint-id <endpoint-id> [--name <name>] [--description <description>] [--tags <tags>] [--proxies <proxies>] [--status <status>] 更新一个endpoint
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
1. llmpool-ctl apikeys list --user <username_or_id>，显示用户的apikey列表
1. llmpool-ctl apikeys add --user <username_or_id> --label <label>，新增一个apikey
1. llmpool-ctl user show --user <username_or_id>, 显示用户信息

=========
AccessKey 修改为 OpenAIAPIKey, 直接修改migraion 文件，不用新建migration文件。

ADMIN api 增加如下命令
1. PUT /api/v1/users/:user_id, 修改用户信息，可修改字段为 username, is_active

llmpool-ctl 添加以下命令
1. llmpool-ctl user update --user <username_or_id> [--username <username>] [--is-active <is-active>]
