# 任务清单 - Key-Value DB 项目成熟化

- [ ] 1. 实现命令行参数与配置管理模块（FR-010）
  - 新增 `src/config.rs` 模块，定义 `KvConfig` 结构体，包含字段：`db_path`（默认 `"data/kvstore.db"`）、`buffer_pool_size`（默认 `1000`）、`btree_order`（默认 `64`）、`config_file`、`batch_file`、`stop_on_error`、`repair_mode`
  - 实现 `KvConfig::from_args()` 方法，使用 `std::env::args()` 手动解析 `--db-path`、`--buffer-pool-size`、`--btree-order`、`--config`、`--file`、`--stop-on-error`、`--repair`、`--help` 参数
  - 修改 `KVEngine` 新增 `open_with_config(config: &KvConfig) -> Result<KVEngine>` 方法，替代 `open()` 中的硬编码值（缓冲池大小 `1000`、B+ 树阶数 `64`）
  - 保留 `KVEngine::open(db_path)` 便利方法，内部使用默认配置
  - 修改 `src/main.rs`，使用 `KvConfig::from_args()` 解析参数并调用 `open_with_config`
  - 实现 `--help` 参数输出所有可用参数说明
  - 在 `src/lib.rs` 中注册 `pub mod config`
  - 确保子需求可独立运行
  - _需求：[FR-010]_

- [x] 2. 实现键值大小限制校验与错误类型扩展（FR-012, FR-013）
  - 在 `src/error.rs` 的 `KvError` 枚举中新增变体：`KeyTooLarge { key_size: usize, max_size: usize }`、`ValueTooLarge { value_size: usize, max_size: usize }`、`InvalidArgument { argument: String }`、`DatabaseVersionMismatch { expected: u32, actual: u32 }`、`TransactionTimeout { txn_id: u64 }`、`ConfigError { message: String }`、`BatchError { line_number: usize, message: String }`
  - 为所有新增错误变体实现 `Display` trait，保持英文描述
  - 修改 `src/kv_engine.rs` 的 `put()` 方法，在执行任何操作前校验 key 和 value 长度：定义常量 `MAX_KEY_SIZE = 4060`、`MAX_VALUE_SIZE = 4060`，超限时返回 `KeyTooLarge` 或 `ValueTooLarge` 错误
  - 在 `src/repl.rs` 中建立 `KvError → 中文描述` 的翻译函数，将库级别英文错误信息翻译为包含上下文的中文消息（如 `put "xxx" 失败: 键大小 5000 字节超过最大限制 4060 字节`）
  - 替换 REPL 中所有 `eprintln!("错误: {}", e)` 为调用中文翻译函数
  - 确保子需求可独立运行
  - _需求：[FR-012, FR-013]_

- [x] 3. 实现非事务写入的 WAL 记录（FR-001）
  - 修改 `src/kv_engine.rs` 的 `put()` 方法非事务分支：在 `btree.insert()` 前通过 `txn_mgr` 分配临时事务 ID，写入 `WALRecord { op_type: Put, txn_id: auto_id, ... }`，执行插入后写入 `WALRecord { op_type: Commit, txn_id: auto_id }`
  - 修改 `src/kv_engine.rs` 的 `delete()` 方法非事务分支：同样包装为自动事务，写入 `WALRecord { op_type: Delete }` 和 `Commit` 记录
  - 在 `src/transaction.rs` 的 `TransactionManager` 中新增 `auto_txn_id()` 方法返回并递增 `next_txn_id`，以及 `write_wal_record()` 方法用于写入单条 WAL 记录
  - 不创建 `Transaction` 对象，仅使用 WALManager 写入记录
  - 确保 `KVEngine::put()` 和 `KVEngine::delete()` 的公共 API 签名不变
  - 确保子需求可独立运行
  - _需求：[FR-001]_

- [ ] 4. 实现前缀扫描功能（FR-005）
  - 在 `src/btree.rs` 的 `BPlusTree` 中新增 `prefix_scan(prefix: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)>` 方法：计算前缀上界（最后一个字节 +1，若为 0xFF 则追加 0x00），从 `find_leaf` 定位起始叶子节点，遍历叶子链表，在每个节点内检查 key 是否以 prefix 开头，不匹配时立即停止
  - 在 `src/kv_engine.rs` 新增 `prefix_scan(prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>` 公共方法
  - 在 `src/repl.rs` 新增 `prefix <prefix>` 命令处理，展示匹配的键值对列表
  - 确保子需求可独立运行
  - _需求：[FR-005]_

- [ ] 5. 实现逆序扫描功能（FR-006）
  - 在 `src/btree.rs` 的 `BPlusTreeNode` 结构体新增 `prev_leaf: Option<u32>` 字段
  - 修改 `serialize()` 和 `deserialize()` 方法：在 `next_leaf` 之后追加/解析 `prev_leaf` 字段（4 字节，0xFFFFFFFF 表示 None）
  - 修改叶子节点分裂逻辑（`insert_recursive`）：右节点的 `prev_leaf` 指向左节点，若右节点原 `next_leaf` 指向的节点其 `prev_leaf` 更新为右节点
  - 修改叶子节点合并逻辑（`merge_nodes`）：正确维护 `prev_leaf` 指针关系
  - 在 `BPlusTree` 新增 `range_scan_reverse(start: &[u8], end: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)>` 方法：通过 `find_leaf` 定位 end key 所在叶子节点，通过 `prev_leaf` 向左遍历，每个节点内从右向左收集 key
  - 在 `src/kv_engine.rs` 新增 `scan_reverse(start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>` 公共方法
  - 在 `src/repl.rs` 新增 `rscan <start> <end>` 命令处理
  - 确保子需求可独立运行
  - _需求：[FR-006]_

- [ ] 6. 实现带分页的范围扫描功能（FR-007）
  - 在 `src/btree.rs` 的 `BPlusTree` 新增 `range_scan_with_limit(start: &[u8], end: &[u8], limit: usize, offset: usize) -> Vec<(Vec<u8>, Vec<u8>)>` 方法：正常遍历叶子节点，跳过前 `offset` 个匹配项，收集最多 `limit` 个后立即停止遍历
  - 在 `src/kv_engine.rs` 新增 `scan_with_limit(start: &[u8], end: &[u8], limit: usize, offset: usize) -> Result<Vec<(Vec<u8>, Vec<u8>)>>` 公共方法
  - 修改 `src/repl.rs` 的 `scan` 命令处理，支持 `scan <start> <end> [limit] [offset]` 可选参数格式
  - 确保子需求可独立运行
  - _需求：[FR-007]_

- [ ] 7. 实现键存在性检查功能（FR-008）
  - 在 `src/btree.rs` 的 `BPlusTree` 新增 `exists(key: &[u8]) -> bool` 方法：复用 `search_in_node` 遍历逻辑，在叶子节点找到匹配 key 后立即返回 `true`，不 clone value；遍历到底未找到返回 `false`
  - 在 `src/kv_engine.rs` 新增 `exists(key: &[u8]) -> Result<bool>` 公共方法
  - 在 `src/repl.rs` 新增 `exists <key>` 命令处理，输出键存在/不存在的中文提示
  - 确保子需求可独立运行
  - _需求：[FR-008]_

- [ ] 8. 实现键计数统计功能（FR-009）
  - 在 `src/btree.rs` 的 `BPlusTree` 新增 `count_all() -> usize` 方法：从根节点沿 `children[0]` 下降到最左叶子节点，沿 `next_leaf` 链遍历所有叶子节点，累加每个节点的 `keys.len()`
  - 在 `src/btree.rs` 的 `BPlusTree` 新增 `count_range(start: &[u8], end: &[u8]) -> usize` 方法：类似 `range_scan` 逻辑但只计数不收集数据
  - 在 `src/kv_engine.rs` 新增 `count() -> Result<usize>` 和 `count_range(start: &[u8], end: &[u8]) -> Result<usize>` 公共方法
  - 在 `src/repl.rs` 新增 `count` 和 `count <start> <end>` 命令处理
  - 确保子需求可独立运行
  - _需求：[FR-009]_

- [ ] 9. 实现命令历史记录功能（FR-015）
  - 在 `src/repl.rs` 的 `Repl` 结构体新增 `history: Vec<String>` 和 `history_index: usize` 字段
  - 实现历史记录加载：`Repl::new()` 时从 `{db_dir}/.kvdb_history` 文件读取历史命令
  - 实现历史记录保存：每次执行命令后将命令追加写入 `.kvdb_history` 文件
  - 实现方向键检测：将 `stdin.read_line()` 替换为逐字节读取模式，检测 `\x1b[A`（上箭头）和 `\x1b[B`（下箭头）转义序列，替换当前输入为历史命令
  - 历史记录最大条数设为 1000，超出时淘汰最早记录
  - 确保子需求可独立运行
  - _需求：[FR-015]_

- [ ] 10. 实现数据导出与导入功能（FR-017）
  - 在 `src/kv_engine.rs` 新增 `export(file_path: &str) -> Result<usize>` 方法：全量 `scan` 所有键值对，逐条以 `key<TAB>value` 格式写入文件，非 UTF-8 安全字符使用 `\xHH` 转义，文件头部包含 `#` 注释标识格式和导出时间
  - 在 `src/kv_engine.rs` 新增 `import(file_path: &str) -> Result<ImportStats>` 方法：逐行读取文件跳过注释和空行，按 `<TAB>` 分割 key/value 并反转义，调用 `put` 写入
  - 定义 `ImportStats` 结构体：`total: usize, success: usize, failed: usize`
  - 在 `src/repl.rs` 新增 `export <file_path>` 和 `import <file_path>` 命令处理
  - 确保子需求可独立运行
  - _需求：[FR-017]_

- [ ] 11. 实现优雅退出与数据保护（FR-018）
  - 修改 `src/repl.rs` 的 `exit` 命令处理：检查 `engine.in_transaction()`，若为 `true` 则提示 `"存在未提交事务，请选择：[C]提交 / [R]回滚 / [Esc]取消"`
  - 读取用户输入：`C/c` 调用 `engine.commit()` 后退出；`R/r` 调用 `engine.rollback()` 后退出；其他输入取消退出回到 REPL 循环
  - 无未提交事务时 `exit` 行为与当前一致
  - 确保子需求可独立运行
  - _需求：[FR-018]_

- [ ] 12. 实现事务脏页追踪优化（FR-020）
  - 在 `src/transaction.rs` 的 `Transaction` 结构体新增 `modified_pages: std::collections::HashSet<u32>` 字段
  - 修改 `src/btree.rs` 的 `write_node()` 方法返回 `Result<u32>`（返回写入的 page_id），所有调用处适配新返回值
  - 修改 `TransactionManager::record_operation()`：在执行 B+ 树操作后收集被修改的 page_id 到 `modified_pages`
  - 修改 `TransactionManager::commit()`：仅遍历 `modified_pages` 逐个调用 `flush_page(page_id)`，替代 `flush_all()`
  - 确保子需求可独立运行
  - _需求：[FR-020]_

- [ ] 13. 实现页面空闲回收机制（FR-004）
  - 在 `src/disk_manager.rs` 的 `DiskManager` 新增 `free_list: Vec<u32>` 字段
  - 修改 `alloc_page()` 方法：优先从 `free_list` 弹出页号分配，`free_list` 为空时才递增 `next_page_id`
  - 新增 `free_page(page_id: u32) -> Result<()>` 方法：将页号加入 `free_list`，向该页面写入 `PageType::Free` 标记（0x00）并清零数据区
  - 修改 `.meta` 文件格式：在现有 `[root_page_id: 4B]` 之后追加 `[checkpoint_lsn: 8B]`（预留位置）和 `[free_count: 4B][page_ids: N*4B]`
  - 修改 `DiskManager::new()` 从 `.meta` 文件读取并恢复 `free_list`
  - 新增 `save_metadata()` 方法：将 `free_list` 持久化到 `.meta` 文件
  - 在 `src/buffer_pool.rs` 新增 `free_page(page_id: u32) -> Result<()>` 方法，转发给 `DiskManager::free_page()`
  - 修改 `src/btree.rs` 的节点合并逻辑（`merge_nodes`）：合并后释放被合并的空节点页面，调用 `buffer_pool.free_page()`
  - 确保子需求可独立运行
  - _需求：[FR-004]_

- [ ] 14. 实现数据库文件完整性校验功能（FR-003）
  - 在 `src/kv_engine.rs` 新增 `verify() -> Result<VerifyReport>` 方法
  - 定义 `VerifyReport` 结构体：`total_pages: u32`、`corrupted_pages: Vec<u32>`、`btree_errors: Vec<String>`、`wal_errors: Vec<String>`、`passed: bool`
  - 实现页面 CRC 校验：通过 `DiskManager` 获取总页数，逐页读取（绕过缓冲池直接读磁盘），使用 `Page::deserialize()` 校验 CRC
  - 实现 B+ 树结构校验：从根节点 BFS 遍历，校验内部节点的 children 页号对应的子节点 parent 指向正确、节点内 keys 按字节序升序排列
  - 实现叶子链表连通性校验：从最左叶子沿 `next_leaf` 遍历，验证所有叶子节点可达
  - 实现 WAL 记录校验：读取 WAL 文件逐条反序列化，记录 CRC 失败的 LSN
  - 在 `src/repl.rs` 新增 `verify` 命令处理，输出校验报告
  - 确保子需求可独立运行
  - _需求：[FR-003]_

- [ ] 15. 实现批量操作接口与键重命名（FR-024, FR-025）
  - 在 `src/kv_engine.rs` 新增 `batch_put(items: Vec<(Vec<u8>, Vec<u8>)>) -> Result<usize>` 方法：自动开启事务，逐条执行 `put`（含 FR-012 的键值校验），任一条失败则回滚整个事务，全部成功则提交并返回写入条数
  - 在 `src/kv_engine.rs` 新增 `batch_delete(keys: Vec<Vec<u8>>) -> Result<usize>` 方法：自动开启事务，逐条执行 `delete`，任一条失败则回滚，全部成功则提交并返回删除条数
  - 在 `src/kv_engine.rs` 新增 `rename(old_key: &[u8], new_key: &[u8]) -> Result<()>` 方法：`get(old_key)` 获取旧值，旧键不存在返回 `KeyNotFound`，`put(new_key, old_value)` 写入新键，`delete(old_key)` 删除旧键；非事务模式作为自动事务执行（依赖 FR-001）
  - 在 `src/repl.rs` 新增 `load <file_path>` 命令（从文件逐行读取键值对并批量写入）、`rename <old_key> <new_key>` 命令
  - 确保子需求可独立运行
  - _需求：[FR-024, FR-025]_

- [ ] 16. 实现 WAL 检查点（Checkpoint）机制（FR-002）
  - 在 `src/wal.rs` 的 `WALManager` 新增 `truncate_before(lsn: u64) -> Result<()>` 方法：读取所有 WAL 记录，筛选 LSN > lsn 的记录，清空 WAL 文件后重写保留记录
  - 在 `src/wal.rs` 新增 `get_current_lsn() -> u64` 方法
  - 修改 `src/btree.rs` 的 `save_metadata()` 方法：在 `.meta` 文件中写入 `checkpoint_lsn` 字段（`[root_page_id: 4B][checkpoint_lsn: 8B][free_list...]`）
  - 修改 `src/btree.rs` 的 `load_root_from_disk()` 方法：从 `.meta` 读取 `checkpoint_lsn`
  - 在 `src/kv_engine.rs` 新增 `checkpoint() -> Result<()>` 方法：调用 `flush_all()` 刷写脏页 → 获取当前 LSN → 调用 `truncate_before()` 截断 WAL → 调用 `save_metadata()` 保存 checkpoint_lsn
  - 修改 `src/recovery.rs` 的 `recover_from_wal()` 方法：从 `.meta` 读取 `checkpoint_lsn`，仅重放 LSN > checkpoint_lsn 的 WAL 记录
  - 在 `src/repl.rs` 新增 `checkpoint` 命令处理
  - 确保子需求可独立运行
  - _需求：[FR-002]_
  - _依赖：任务 3_

- [ ] 17. 实现配置文件支持（FR-011）
  - 在 `src/config.rs` 新增 `KvConfig::load_from_file(path: &str) -> Result<KvConfig>` 方法：逐行解析 `key = value` 格式，`#` 开头和空行跳过，value 自动去除首尾空白，数值字段自动类型转换
  - 修改 `KvConfig::from_args()`：先加载默认值 → 若指定 `--config` 则读取配置文件覆盖 → 命令行参数覆盖配置文件值
  - 配置文件可设置字段：`db_path`、`buffer_pool_size`、`btree_order`、`default_timeout_secs`
  - 配置文件格式错误时返回 `KvError::ConfigError`，包含文件名、行号和错误原因
  - 配置文件不存在时使用默认值并输出提示信息
  - 确保子需求可独立运行
  - _需求：[FR-011]_
  - _依赖：任务 1_

- [ ] 18. 实现批处理文件执行功能（FR-016）
  - 在 `src/repl.rs` 新增 `execute_line(&mut self, line: &str) -> crate::error::Result<bool>` 公共方法，将单行 tokenize 后调用 `execute()`
  - 在 `src/repl.rs` 新增 `execute_batch(file_path: &str, stop_on_error: bool) -> Result<BatchStats>` 方法：打开文件逐行读取，跳过空行和 `#` 注释行，调用 `execute_line()`，记录成功/失败计数
  - 定义 `BatchStats` 结构体：`success_count: usize, fail_count: usize`
  - 修改 `src/main.rs`：判断 `--file` 参数，有则调用 `execute_batch()` 后退出，无则进入交互模式
  - 批处理完成后输出 `"执行完成: 成功 N 条, 失败 M 条"` 摘要
  - 确保子需求可独立运行
  - _需求：[FR-016]_
  - _依赖：任务 1_

- [ ] 19. 实现操作结果反馈增强（FR-014）
  - 修改 `src/repl.rs` 的 `cmd_put()`：调用前通过 `engine.exists()` 检查 key 是否已存在，新键输出 `"OK - 已插入 (key: xxx)"`，覆盖输出 `"OK - 已更新 (key: xxx)"`
  - 修改 `src/repl.rs` 的 `cmd_delete()`：输出 `"OK - 已删除 (key: xxx)"`，包含被删除的键名
  - 修改 `src/repl.rs` 的 `cmd_status()`：扩展展示信息包括数据库路径、键总数（调用 `engine.count()`）、已使用页数/总页数（通过 `DiskManager` 统计）、缓冲池使用率（frames_used / capacity）、当前事务 ID（若有）、WAL 文件大小、数据文件大小
  - 修改 `src/repl.rs` 的 `cmd_scan()`：首行显示扫描范围，末行显示总条数
  - 在 `src/kv_engine.rs` 或 `src/btree.rs` 新增获取页数统计的辅助方法
  - 确保子需求可独立运行
  - _需求：[FR-014]_
  - _依赖：任务 8_

- [ ] 20. 实现运行时统计信息功能（FR-022）
  - 在 `src/kv_engine.rs` 新增 `StatsCollector` 结构体：`put_count: u64`、`get_count: u64`、`delete_count: u64`、`scan_count: u64`、`buffer_hits: u64`、`buffer_misses: u64`
  - 在 `KVEngine` 结构体中新增 `stats: StatsCollector` 字段
  - 修改 `KVEngine` 的 `put/get/delete/scan` 方法：每次调用时递增对应计数器
  - 在 `src/buffer_pool.rs` 的 `fetch_page()` 中区分命中/未命中情况，新增 `get_hit_count() -> u64` 和 `get_miss_count() -> u64` 方法
  - 在 `src/kv_engine.rs` 新增 `get_stats() -> &StatsCollector` 和 `reset_stats()` 公共方法
  - 在 `src/repl.rs` 新增 `stats` 命令展示操作计数和缓冲池命中率，新增 `stats reset` 命令重置计数器
  - 确保子需求可独立运行
  - _需求：[FR-022]_
  - _依赖：任务 8_

- [ ] 21. 实现事务自动回滚超时机制（FR-019）
  - 在 `src/transaction.rs` 的 `Transaction` 结构体新增 `timeout_at: Option<std::time::Instant>` 字段
  - 修改 `TransactionManager::begin()` 方法签名，新增 `timeout_secs: Option<u64>` 参数：`Some(secs)` 时计算 `Instant::now() + Duration::from_secs(secs)` 存入 `timeout_at`
  - 在 `record_operation()` 执行前检查：若 `timeout_at` 已过期，调用 `rollback()` 并返回 `KvError::TransactionTimeout`
  - 修改 `src/kv_engine.rs` 的 `begin()` 方法适配新签名（默认传 `None`）
  - 修改 `src/repl.rs` 的 `cmd_begin()` 解析可选 `--timeout N` 参数：`begin --timeout 30` 传入 `Some(30)`
  - 配置文件集成：从 `KvConfig` 的 `default_timeout_secs` 读取默认超时（0 表示无超时）
  - 确保子需求可独立运行
  - _需求：[FR-019]_
  - _依赖：任务 17_

- [ ] 22. 实现数据库空间紧凑化功能（FR-021）
  - 在 `src/kv_engine.rs` 新增 `compact() -> Result<CompactionStats>` 方法
  - 定义 `CompactionStats` 结构体：`original_size: u64`、`compacted_size: u64`、`reclaimed_size: u64`、`pages_before: u32`、`pages_after: u32`
  - 紧凑化流程：全量 scan 当前 B+ 树键值对 → 在临时文件路径创建新的 `DiskManager → BufferPoolManager → BPlusTree` 实例 → 依次插入所有键值对 → 刷写新数据库文件 → 原文件重命名为 `.bak` → 新文件重命名为原文件名 → 删除 `.bak`
  - 紧凑化过程中出错时不破坏原文件（所有写操作在新文件上进行）
  - 在 `src/repl.rs` 新增 `compact` 命令处理，输出前后文件大小对比和回收空间
  - 确保子需求可独立运行
  - _需求：[FR-021]_
  - _依赖：任务 13_

- [ ] 23. 实现数据库重建/修复工具（FR-023）
  - 在 `src/kv_engine.rs` 新增 `repair() -> Result<RepairReport>` 方法
  - 定义 `RepairReport` 结构体：`skipped_pages: Vec<u32>`、`recovered_records: usize`、`lost_records: usize`
  - 实现 `.meta` 文件丢失时的元数据重建算法：读取所有页面按类型分类（Internal/Leaf/Free），构建 parent→children 映射，找到无 parent 的节点为根节点，重建叶子节点链表（按 key 范围排序），写入新 `.meta` 文件
  - 实现部分页面损坏修复：跳过损坏页面（CRC 校验失败的页），从 WAL 日志尝试恢复可恢复数据
  - 实现 WAL 全量重建策略：清空 B+ 树，从 WAL 的已提交事务重放所有操作
  - 在 `src/main.rs` 中处理 `--repair` 参数：调用 `repair()` 并输出详细修复日志
  - 修复完成后验证数据库可正常打开和使用
  - 确保子需求可独立运行
  - _需求：[FR-023]_
  - _依赖：任务 3, 任务 14_
