use crate::error::KvError;
use crate::KVEngine;
use std::io::{self, Write};
use std::time::Instant;

/// REPL 交互式命令行
pub struct Repl {
    engine: KVEngine,
}

/// 将 KvError 翻译为包含上下文的中文错误消息
fn translate_error(err: &KvError) -> String {
    match err {
        KvError::PageCorrupted { page_id } => {
            format!("数据页损坏: 页 {} CRC 校验失败", page_id)
        }
        KvError::PageNotFound { page_id } => {
            format!("页不存在: 页 {}", page_id)
        }
        KvError::KeyNotFound => "键不存在".to_string(),
        KvError::IoError(e) => format!("I/O 错误: {}", e),
        KvError::WalCorrupted { lsn } => {
            format!("WAL 日志损坏: LSN {} CRC 校验失败", lsn)
        }
        KvError::NoActiveTransaction => "无活跃事务".to_string(),
        KvError::NestedTransaction => "不支持嵌套事务".to_string(),
        KvError::DatabaseClosed => "数据库已关闭".to_string(),
        KvError::BufferPoolFull => "缓冲池已满，无法分配新帧".to_string(),
        KvError::KeyTooLarge { key_size, max_size } => {
            format!(
                "键大小 {} 字节超过最大限制 {} 字节",
                key_size, max_size
            )
        }
        KvError::ValueTooLarge {
            value_size,
            max_size,
        } => {
            format!(
                "值大小 {} 字节超过最大限制 {} 字节",
                value_size, max_size
            )
        }
        KvError::InvalidArgument { argument } => {
            format!("无效参数: {}", argument)
        }
        KvError::DatabaseVersionMismatch { expected, actual } => {
            format!(
                "数据库版本不匹配: 期望版本 {}，实际版本 {}",
                expected, actual
            )
        }
        KvError::TransactionTimeout { txn_id } => {
            format!("事务 {} 超时，已自动回滚", txn_id)
        }
        KvError::ConfigError { message } => {
            format!("配置错误: {}", message)
        }
        KvError::BatchError {
            line_number,
            message,
        } => {
            format!("批处理第 {} 行错误: {}", line_number, message)
        }
    }
}

impl Repl {
    /// 创建新的 REPL 实例
    pub fn new(engine: KVEngine) -> Self {
        Repl { engine }
    }

    /// 启动 REPL 主循环
    pub fn run(&mut self) {
        println!("🔑 Key-Value DB 交互式命令行");
        println!("输入 help 查看可用命令，exit 退出\n");

        let stdin = io::stdin();
        loop {
            // 显示提示符（事务中时带 * 标记）
            let prompt = if self.engine.in_transaction() {
                "kvdb(txn)*> "
            } else {
                "kvdb> "
            };
            print!("{}", prompt);
            io::stdout().flush().unwrap();

            // 读取一行输入
            let mut line = String::new();
            match stdin.read_line(&mut line) {
                Ok(0) => {
                    // EOF (Ctrl+D / Ctrl+Z)
                    println!();
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("读取输入失败: {}", e);
                    break;
                }
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // 解析并执行命令
            let tokens = Self::tokenize(line);
            if tokens.is_empty() {
                continue;
            }

            let cmd = tokens[0].to_lowercase();
            let result = self.execute(&cmd, &tokens[1..]);

            match result {
                Ok(true) => break,        // exit
                Ok(false) => {}           // 继续循环
                Err(e) => eprintln!("错误: {}", translate_error(&e)),
            }
        }

        // 关闭数据库
        if self.engine.is_open() {
            if let Err(e) = self.engine.close() {
                eprintln!("关闭数据库失败: {}", translate_error(&e));
            } else {
                println!("数据库已关闭。");
            }
        }
    }

    /// 执行命令
    /// 返回 Ok(true) 表示应该退出
    fn execute(&mut self, cmd: &str, args: &[String]) -> crate::error::Result<bool> {
        match cmd {
            "put" | "set" | "insert" => {
                self.cmd_put(args)?;
                Ok(false)
            }
            "get" | "query" => {
                self.cmd_get(args)?;
                Ok(false)
            }
            "delete" | "del" | "remove" | "rm" => {
                self.cmd_delete(args)?;
                Ok(false)
            }
            "scan" | "range" => {
                self.cmd_scan(args)?;
                Ok(false)
            }
            "prefix" => {
                self.cmd_prefix(args)?;
                Ok(false)
            }
            "begin" | "begin_txn" | "transaction" | "txn" => {
                self.cmd_begin()?;
                Ok(false)
            }
            "commit" => {
                self.cmd_commit()?;
                Ok(false)
            }
            "rollback" | "abort" => {
                self.cmd_rollback()?;
                Ok(false)
            }
            "status" | "info" => {
                self.cmd_status();
                Ok(false)
            }
            "help" | "?" => {
                Self::cmd_help();
                Ok(false)
            }
            "exit" | "quit" | "q" => {
                println!("再见！");
                Ok(true)
            }
            _ => {
                println!("未知命令: '{}'，输入 help 查看可用命令", cmd);
                Ok(false)
            }
        }
    }

    /// PUT 命令：put <key> <value>
    fn cmd_put(&mut self, args: &[String]) -> crate::error::Result<()> {
        if args.len() < 2 {
            println!("用法: put <key> <value>");
            println!("示例: put name Alice");
            println!("      put \"my key\" \"my value\"");
            return Ok(());
        }
        let key = &args[0];
        let value = &args[1];

        // 检查 key 是否已存在以提供更详细的反馈
        let existed = self
            .engine
            .get(key.as_bytes())
            .unwrap_or(None)
            .is_some();

        let start = Instant::now();
        self.engine.put(key.as_bytes(), value.as_bytes())?;
        let elapsed = start.elapsed();

        if existed {
            println!(
                "OK - 已更新 (key: \"{}\", 耗时: {:.2?})",
                key, elapsed
            );
        } else {
            println!(
                "OK - 已插入 (key: \"{}\", 耗时: {:.2?})",
                key, elapsed
            );
        }
        Ok(())
    }

    /// GET 命令：get <key>
    fn cmd_get(&mut self, args: &[String]) -> crate::error::Result<()> {
        if args.len() < 1 {
            println!("用法: get <key>");
            return Ok(());
        }
        let key = &args[0];
        let start = Instant::now();
        match self.engine.get(key.as_bytes())? {
            Some(value) => {
                let elapsed = start.elapsed();
                let s = String::from_utf8_lossy(&value);
                println!(
                    "{} (key: \"{}\", 大小: {} 字节, 耗时: {:.2?})",
                    s,
                    key,
                    value.len(),
                    elapsed
                );
            }
            None => {
                println!("(nil) - 键 \"{}\" 不存在", key);
            }
        }
        Ok(())
    }

    /// DELETE 命令：delete <key>
    fn cmd_delete(&mut self, args: &[String]) -> crate::error::Result<()> {
        if args.len() < 1 {
            println!("用法: delete <key>");
            return Ok(());
        }
        let key = &args[0];
        let start = Instant::now();
        let deleted = self.engine.delete(key.as_bytes())?;
        let elapsed = start.elapsed();
        if deleted {
            println!(
                "OK - 已删除 (key: \"{}\", 耗时: {:.2?})",
                key, elapsed
            );
        } else {
            println!("(nil) - 键 \"{}\" 不存在", key);
        }
        Ok(())
    }

    /// SCAN 命令：scan <start> <end>
    fn cmd_scan(&mut self, args: &[String]) -> crate::error::Result<()> {
        if args.len() < 2 {
            println!("用法: scan <start_key> <end_key>");
            println!("示例: scan a z");
            println!("      scan user:0001 user:0010");
            return Ok(());
        }
        let start = &args[0];
        let end = &args[1];

        println!("扫描范围: [\"{}\", \"{}\"]", start, end);
        let timer = Instant::now();
        let results = self.engine.scan(start.as_bytes(), end.as_bytes())?;
        let elapsed = timer.elapsed();

        if results.is_empty() {
            println!("(empty) - 范围内无数据");
        } else {
            for (i, (key, value)) in results.iter().enumerate() {
                let k = String::from_utf8_lossy(key);
                let v = String::from_utf8_lossy(value);
                println!("{}) {} => {}", i + 1, k, v);
            }
            println!(
                "共 {} 条记录 (耗时: {:.2?})",
                results.len(),
                elapsed
            );
        }
        Ok(())
    }

    /// BEGIN 命令
    fn cmd_begin(&mut self) -> crate::error::Result<()> {
        let txn_id = self.engine.begin()?;
        println!("事务已开启 (txn_id: {})", txn_id);
        Ok(())
    }

    /// COMMIT 命令
    fn cmd_commit(&mut self) -> crate::error::Result<()> {
        let start = Instant::now();
        self.engine.commit()?;
        let elapsed = start.elapsed();
        println!("事务已提交 (耗时: {:.2?})", elapsed);
        Ok(())
    }

    /// ROLLBACK 命令
    fn cmd_rollback(&mut self) -> crate::error::Result<()> {
        let start = Instant::now();
        self.engine.rollback()?;
        let elapsed = start.elapsed();
        println!("事务已回滚 (耗时: {:.2?})", elapsed);
        Ok(())
    }

    /// STATUS 命令
    fn cmd_status(&self) {
        println!("数据库状态:");
        println!(
            "  已打开: {}",
            if self.engine.is_open() {
                "是"
            } else {
                "否"
            }
        );
        println!(
            "  事务中: {}",
            if self.engine.in_transaction() {
                "是"
            } else {
                "否"
            }
        );
    }

    /// PREFIX 命令：prefix <prefix>
    fn cmd_prefix(&mut self, args: &[String]) -> crate::error::Result<()> {
        if args.len() < 1 {
            println!("用法: prefix <前缀>");
            println!("示例: prefix user:");
            println!("      prefix session_");
            return Ok(());
        }
        let prefix = &args[0];

        println!("前缀扫描: \"{}\"", prefix);
        let timer = Instant::now();
        let results = self.engine.prefix_scan(prefix.as_bytes())?;
        let elapsed = timer.elapsed();

        if results.is_empty() {
            println!("(empty) - 无匹配数据");
        } else {
            for (i, (key, value)) in results.iter().enumerate() {
                let k = String::from_utf8_lossy(key);
                let v = String::from_utf8_lossy(value);
                println!("{}) {} => {}", i + 1, k, v);
            }
            println!(
                "共 {} 条记录 (耗时: {:.2?})",
                results.len(),
                elapsed
            );
        }
        Ok(())
    }

    /// HELP 命令
    fn cmd_help() {
        println!();
        println!("╔══════════════════════════════════════════════════╗");
        println!("║          🔑 Key-Value DB 命令列表               ║");
        println!("╠══════════════════════════════════════════════════╣");
        println!("║  put <key> <value>     插入或更新键值对          ║");
        println!("║  get <key>             获取键对应的值            ║");
        println!("║  delete <key>          删除键                    ║");
        println!("║  scan <start> <end>    范围扫描                  ║");
        println!("║  prefix <prefix>       前缀扫描                  ║");
        println!("║  begin                 开启事务                  ║");
        println!("║  commit                提交当前事务              ║");
        println!("║  rollback              回滚当前事务              ║");
        println!("║  status                查看数据库状态            ║");
        println!("║  help                  显示帮助信息              ║");
        println!("║  exit                  退出程序                  ║");
        println!("╠══════════════════════════════════════════════════╣");
        println!("║  别名:                                           ║");
        println!("║    put = set, insert                             ║");
        println!("║    get = query                                   ║");
        println!("║    delete = del, remove, rm                      ║");
        println!("║    scan = range                                  ║");
        println!("║    begin = txn, transaction                      ║");
        println!("║    rollback = abort                              ║");
        println!("║    help = ?                                      ║");
        println!("║    exit = quit, q                                ║");
        println!("╠══════════════════════════════════════════════════╣");
        println!("║  提示:                                           ║");
        println!("║    - 使用双引号包裹含空格的 key 或 value          ║");
        println!("║    - 事务中提示符变为 kvdb(txn)*>                ║");
        println!("╚══════════════════════════════════════════════════╝");
        println!();
    }

    /// 简单的命令行分词器，支持双引号
    fn tokenize(input: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut chars = input.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                }
                ' ' | '\t' if !in_quotes => {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                }
                _ => {
                    current.push(ch);
                }
            }
        }

        if !current.is_empty() {
            tokens.push(current);
        }

        tokens
    }
}
