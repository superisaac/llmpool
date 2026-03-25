

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