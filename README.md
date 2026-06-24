# 🔑 Key-Value DB

一个基于 **B+ 树**索引的嵌入式键值存储引擎，使用 Rust 实现。采用经典的数据库存储引擎分层架构，支持 ACID 事务、崩溃恢复与范围查询。

## ✨ 特性

- **B+ 树索引** — 支持高效的点查与范围扫描
- **缓冲池管理** — LRU 淘汰策略，减少磁盘 I/O
- **WAL 预写日志** — 保证崩溃后数据可恢复
- **ACID 事务** — 支持 Begin / Commit / Rollback
- **崩溃恢复** — 自动重放已提交事务，撤销未提交事务
- **CRC32 校验** — 页面与 WAL 记录均包含完整性校验
- **交互式 REPL** — 内置命令行界面，支持 put/get/delete/scan 等操作
- **零外部依赖** — 仅使用 Rust 标准库 + tempfile（测试用）

## 🏗️ 架构

```
┌─────────────────────────────────────────────┐
│                  KVEngine                    │
│            (对外核心 API)                     │
├─────────────────────────────────────────────┤
│           TransactionManager                 │
│     (事务生命周期 / 快照 / 回滚)              │
├──────────────────┬──────────────────────────┤
│   WALManager     │        BPlusTree          │
│  (预写式日志)     │     (B+ 树索引)           │
├──────────────────┴──────────────────────────┤
│           RecoveryManager                    │
│         (崩溃恢复 / Redo)                    │
├─────────────────────────────────────────────┤
│          BufferPoolManager                   │
│    (LRU 缓冲池 / 脏页管理 / Pin-Unpin)       │
├─────────────────────────────────────────────┤
│            DiskManager                       │
│      (页面分配 / 文件 I/O)                    │
├─────────────────────────────────────────────┤
│    data/kvstore.db    data/kvstore.wal       │
│         (数据文件)        (WAL 日志)          │
└─────────────────────────────────────────────┘
```

### 模块说明
| 模块 | 文件 | 职责 |
|------|------|------|
| **KVEngine** | [`kv_engine.rs`](src/kv_engine.rs) | 对外暴露的核心 API，协调所有子系统 |
| **Repl** | [`repl.rs`](src/repl.rs) | 交互式命令行界面，支持 REPL 操作 |
| **BPlusTree** | [`btree.rs`](src/btree.rs) | B+ 树索引实现，支持内部节点和叶子节点 |
| **BufferPoolManager** | [`buffer_pool.rs`](src/buffer_pool.rs) | 内存缓冲池，LRU 淘汰策略，脏页管理 |
| **DiskManager** | [`disk_manager.rs`](src/disk_manager.rs) | 磁盘文件 I/O，页面分配与读写 |
| **WALManager** | [`wal.rs`](src/wal.rs) | 预写式日志，记录操作历史用于恢复 |
| **TransactionManager** | [`transaction.rs`](src/transaction.rs) | 事务生命周期管理，快照与回滚 |
| **RecoveryManager** | [`recovery.rs`](src/recovery.rs) | 崩溃恢复，重放已提交事务，撤销未提交事务 |
| **Page** | [`page.rs`](src/page.rs) | 4KB 固定大小页面，包含 CRC32 校验 |
| **Error** | [`error.rs`](src/error.rs) | 统一错误类型定义 |
| **Error** | [`error.rs`](src/error.rs) | 统一错误类型定义 |

### 数据文件

| 文件类型 | 扩展名 | 说明 |
|---------|--------|------|
| 数据文件 | `.db` | 存储 B+ 树节点数据，每页 4KB |
| 元数据文件 | `.meta` | 存储根节点页号等元信息 |
| WAL 日志 | `.wal` | 预写式日志，记录所有写操作 |

## 🚀 快速开始

### 环境要求

- Rust 1.70+ (Edition 2021)
- 支持 Windows / Linux / macOS

### 构建与运行

```bash
# 克隆项目
git clone <repository-url>
cd Key-Value_db

# 构建
cargo build

# 运行（交互式 REPL）
cargo run

# 运行测试
cargo test
```

### 交互式命令行 (REPL)

运行 `cargo run` 后进入交互式 REPL，可以直接操作数据库：

```
🔑 Key-Value DB 交互式命令行
输入 help 查看可用命令，exit 退出

kvdb> put name Alice
OK
kvdb> put age 30
OK
kvdb> get name
Alice
kvdb> scan a z
1) age => 30
2) name => Alice
共 2 条记录
kvdb> delete age
OK - 已删除
kvdb> begin
事务已开启 (txn_id: 1)
kvdb(txn)*> put counter 100
OK
kvdb(txn)*> commit
事务已提交
kvdb> exit
再见！
数据库已关闭。
```

#### REPL 命令列表

| 命令 | 别名 | 说明 |
|------|------|------|
| `put <key> <value>` | set, insert | 插入或更新键值对 |
| `get <key>` | query | 获取键对应的值 |
| `delete <key>` | del, remove, rm | 删除键 |
| `scan <start> <end>` | range | 范围扫描 |
| `begin` | txn, transaction | 开启事务 |
| `commit` | | 提交当前事务 |
| `rollback` | abort | 回滚当前事务 |
| `status` | info | 查看数据库状态 |
| `help` | ? | 显示帮助信息 |
| `exit` | quit, q | 退出程序 |

> 提示：使用双引号包裹含空格的 key 或 value，例如 `put "my key" "my value"`

### 编程方式使用

```rust
use kv_engine::KVEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 打开数据库
    let mut engine = KVEngine::open("data/kvstore.db")?;

    // 插入键值对
    engine.put(b"name", b"Alice")?;
    engine.put(b"age", b"30")?;

    // 查询
    if let Some(value) = engine.get(b"name")? {
        println!("name = {}", String::from_utf8_lossy(&value));
    }

    // 范围扫描
    let results = engine.scan(b"a", b"z")?;
    for (key, value) in results {
        println!("{} => {}", 
            String::from_utf8_lossy(&key),
            String::from_utf8_lossy(&value)
        );
    }

    // 删除
    engine.delete(b"age")?;

    // 关闭数据库
    engine.close()?;
    Ok(())
}
```

### 事务使用

```rust
use kv_engine::KVEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = KVEngine::open("data/kvstore.db")?;

    // 开启事务
    let txn_id = engine.begin()?;
    println!("事务 ID: {}", txn_id);

    // 事务内操作
    engine.put(b"key1", b"value1")?;
    engine.put(b"key2", b"value2")?;

    // 提交事务
    engine.commit()?;

    // 或者回滚
    // engine.rollback()?;

    engine.close()?;
    Ok(())
}
```

## 🔧 技术细节

### B+ 树实现

- **节点类型**：内部节点（Internal）和叶子节点（Leaf）
- **节点容量**：根据阶数（order）动态计算
- **分裂策略**：节点满时自动分裂，维护有序性
- **叶子链表**：叶子节点通过 `next_leaf` 指针连接，支持高效范围扫描
- **序列化**：节点数据序列化后存储在 4KB 页面中

### 缓冲池管理

- **LRU 淘汰**：基于访问时间戳的最近最少使用策略
- **Pin/Unpin**：引用计数机制，防止正在使用的页面被淘汰
- **脏页标记**：修改后的页面标记为 dirty，刷写时写入磁盘
- **容量限制**：可配置缓冲池大小（默认 1000 帧），超出时触发淘汰

### WAL 日志

- **记录格式**：LSN + TxnID + OpType + Key + Value + CRC32
- **操作类型**：Put / Delete / Commit / Rollback
- **CRC32 校验**：每条记录包含校验和，防止日志损坏
- **恢复机制**：重启时重放已提交事务，撤销未提交事务

### 事务管理

- **ACID 支持**：
  - **原子性（Atomicity）**：通过 WAL 和回滚快照保证
  - **一致性（Consistency）**：事务要么全部提交，要么全部回滚
  - **隔离性（Isolation）**：事务内可读取未提交数据（Read Uncommitted）
  - **持久性（Durability）**：提交后数据持久化到磁盘
- **快照机制**：记录修改前的旧值，用于回滚恢复

### 崩溃恢复

- **恢复流程**：
  1. 读取 WAL 日志所有有效记录
  2. 按事务 ID 分组
  3. 识别已提交和未提交事务
  4. 重放已提交事务（Redo）
  5. 跳过未提交事务（Undo 通过回滚实现）
- **统计信息**：记录重放和跳过的记录数

## 🧪 测试覆盖

项目包含 61 个单元测试，覆盖所有核心模块：

- **B+ 树测试**：插入、查询、更新、删除、范围扫描、节点分裂
- **缓冲池测试**：页面创建、获取、淘汰、LRU 顺序、脏页刷写
- **磁盘管理器测试**：页面分配、读写、WAL 操作
- **事务测试**：Begin / Commit / Rollback、嵌套事务错误处理
- **WAL 测试**：记录序列化、追加、恢复、损坏检测
- **恢复测试**：空恢复、已提交事务恢复
- **KVEngine 测试**：完整 CRUD 操作、事务、持久性验证

## 📁 项目结构

```
Key-Value_db/
├── Cargo.toml              # 项目配置与依赖
├── README.md               # 项目文档
├── src/
│   ├── lib.rs              # 库入口，模块导出
│   ├── main.rs             # REPL 交互式命令行入口
│   ├── kv_engine.rs        # KV 存储引擎核心 API
│   ├── repl.rs             # 交互式命令行 (REPL)
│   ├── btree.rs            # B+ 树索引实现
│   ├── buffer_pool.rs      # 缓冲池管理器
│   ├── disk_manager.rs     # 磁盘文件管理器
│   ├── wal.rs              # WAL 预写式日志
│   ├── transaction.rs      # 事务管理器
│   ├── recovery.rs         # 崩溃恢复管理器
│   ├── page.rs             # 4KB 页面结构
│   └── error.rs            # 统一错误类型
└── data/                   # 运行时数据目录
    ├── kvstore.db          # 数据文件
    ├── kvstore.meta        # 元数据文件
    ├── kvstore.wal         # WAL 日志文件
    ├── kvstore_wal.db      # WAL 管理器数据文件
    └── kvstore_wal.wal     # WAL 管理器日志文件
```

## 📄 许可证

本项目仅供学习和研究使用。

