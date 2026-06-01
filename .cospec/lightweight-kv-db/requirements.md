# 轻量级 Key-Value 数据库 - 需求规格说明书

> **版本**: v0.1.0-draft
> **日期**: 2025-06-01
> **状态**: 初稿

---

# Part 1: Proposal

## Proposal: 轻量级 Key-Value 存储引擎

## 背景

数据库内核是计算机科学中最复杂的系统工程之一，涉及存储管理、索引算法、事务处理和崩溃恢复等核心技术。当前市面上的工业级数据库（如 LevelDB、RocksDB、SQLite）虽然开源，但其代码量庞大、优化层次极深，不利于学习者快速理解存储引擎的核心设计思想。

构建一个轻量级的 Key-Value 数据库，能够以最小化的实现覆盖存储引擎的关键路径，包括：

- **磁盘与内存的协同管理**：理解数据如何在内存缓冲区与持久化文件之间流转
- **B+树索引结构**：掌握有序索引的查找、插入、删除以及节点分裂与合并机制
- **ACID 事务模型**：理解事务隔离级别、并发控制和回滚机制
- **WAL（Write-Ahead Log）机制**：理解预写日志如何保证崩溃恢复能力

本项目的核心价值在于"通过实现来学习"——以构建一个可运行的、功能完整的 KV 存储引擎为目标，驱动对数据库内核原理的深入理解。

## 目标

1. **实现一个基于 B+树的 Key-Value 存储引擎**，支持磁盘持久化存储
2. **提供完整的 CRUD 操作接口**，支持单键查询、范围扫描
3. **实现 ACID 事务支持**，包括原子性提交与回滚机制
4. **实现 WAL 预写日志机制**，确保数据在异常崩溃后的可恢复性
5. **代码结构清晰、模块化设计**，每个核心组件（存储层、索引层、事务层、日志层）职责明确，便于理解和扩展

## 范围

### 包含

| 编号 | 功能项 | 说明 |
|------|--------|------|
| S-01 | 内存缓冲区管理 | 数据写入时先进入内存缓冲区，按策略刷写到磁盘 |
| S-02 | 文件持久化存储 | 数据以页（Page）为单位组织存储在磁盘文件中 |
| S-03 | B+树索引 | 支持 B+树的查找、插入、删除，含节点分裂与合并 |
| S-04 | 单键 CRUD | 对单个 Key 的 Put / Get / Delete 操作 |
| S-05 | 范围扫描 | 基于索引的范围查询（Scan）操作 |
| S-06 | 事务管理 | 支持单事务的 Begin / Commit / Rollback |
| S-07 | WAL 预写日志 | 写操作前先记录日志，用于崩溃恢复 |
| S-08 | 崩溃恢复 | 基于 WAL 日志重放（Redo）恢复已提交的数据 |

### 不包含

| 编号 | 排除项 | 排除原因 |
|------|--------|----------|
| X-01 | 多线程/并发控制 | 当前阶段聚焦于单线程下的核心逻辑，不涉及 MVCC 或锁机制 |
| X-02 | 网络服务层 | 不提供 gRPC / HTTP 等远程访问接口 |
| X-03 | SQL 解析层 | 不支持 SQL 查询语言 |
| X-04 | 分布式扩展 | 不涉及分片、复制、一致性协议等分布式特性 |
| X-05 | 多表/命名空间 | 仅支持单一 Key-Value 空间 |
| X-06 | 压缩与编码 | 不支持数据压缩（如 Snappy/ZSTD） |
| X-07 | 备份与快照 | 不支持在线备份或快照功能 |

## 验收标准

| 编号 | 验收标准 | 对应功能 |
|------|----------|----------|
| AC-01 | 支持基本的 Put/Get/Delete 操作，数据可正确持久化到磁盘并在重启后恢复 | S-02, S-04 |
| AC-02 | B+树索引能正确处理节点分裂（插入大量数据后树结构保持平衡） | S-03 |
| AC-03 | B+树索引能正确处理节点合并或借用（删除大量数据后树结构保持平衡） | S-03 |
| AC-04 | 范围扫描能按 Key 有序返回指定范围内的所有键值对 | S-03, S-05 |
| AC-05 | 事务 Commit 后所有写入持久可见，Rollback 后所有写入不可见 | S-06 |
| AC-06 | WAL 日志在写入过程中正确记录，崩溃恢复后数据一致 | S-07, S-08 |
| AC-07 | 模拟崩溃（未正常关闭）后，重启能通过 WAL 日志恢复到最近一致状态 | S-08 |

## 技术栈

| 项目 | 选型 | 说明 |
|------|------|------|
| 编程语言 | Rust | 高性能、内存安全，零成本抽象，适合系统级编程 |
| 运行环境 | 纯软件（单机） | 无需特殊硬件或外部依赖 |
| 存储介质 | 本地文件系统 | 使用标准文件 I/O 进行持久化 |
| 构建工具 | Cargo | Rust 标准构建与包管理工具 |

---

# Part 2: Specs（BDD 格式）

> 本部分使用 RFC 2119 关键词：**MUST**（必须）、**SHALL**（应当）、**SHOULD**（建议）、**MAY**（可以）

---

## Requirement 1: 存储引擎 — 内存缓冲区与文件持久化

The system SHALL provide a storage engine that manages data through an in-memory buffer and persists data to disk files in pages.

### Scenario: 正常写入数据并从磁盘恢复

- GIVEN 存储引擎已初始化，关联的数据文件已打开
- WHEN 用户调用 `Put("key_001", "value_001")` 并触发缓冲区刷写，随后存储引擎关闭并重新打开
- THEN 系统 SHALL 从磁盘文件中正确加载已持久化的数据
- AND 调用 `Get("key_001")` SHALL 返回 `"value_001"`

### Scenario: 数据页校验失败时的错误处理

- GIVEN 磁盘文件中存在一个数据页，其校验信息不匹配（如 CRC 校验失败）
- WHEN 存储引擎尝试读取该数据页
- THEN 系统 SHALL 返回明确的错误指示（而非静默返回错误数据）
- AND 系统 SHALL NOT 使用该损坏页中的任何数据

---

## Requirement 2: B+树索引

The system SHALL implement a B+tree index structure to organize keys in sorted order, supporting efficient point lookups, range scans, and dynamic rebalancing through node splitting and merging.

### Scenario: 精确查找与范围扫描

- GIVEN B+树中已包含 Key `"a"`, `"c"`, `"e"`, `"g"`, `"i"` 及其对应的 Value
- WHEN 用户对 `"e"` 执行精确查找，并执行范围扫描 `Scan("c", "g")`
- THEN 精确查找 SHALL 返回 `"e"` 对应的 Value
- AND 范围扫描 SHALL 按 Key 升序返回 `"c"`, `"e"`, `"g"` 及其对应的 Value

### Scenario: 插入数据触发节点分裂保持树平衡

- GIVEN B+树的某个叶子节点已满（达到阶数上限）
- WHEN 用户连续插入新的 Key-Value 对，导致该叶子节点溢出
- THEN 系统 SHALL 将该叶子节点分裂为两个节点
- AND 中间键（Median Key）SHALL 被提升到父节点
- AND 分裂后 B+树 SHALL 保持平衡（所有叶子节点处于同一深度）
- AND 所有已存在的 Key 在分裂后 SHALL 仍可通过精确查找正确获取

---

## Requirement 3: 基础 CRUD 操作

The system SHALL provide Key-Value CRUD operations including Put, Get, Delete, and Scan, as the primary user-facing interface.

### Scenario: Put/Get/Delete 完整生命周期

- GIVEN 数据库已打开且处于初始状态
- WHEN 用户依次执行 `Put("k1", "v1")`、`Get("k1")`、`Put("k1", "v2")`、`Get("k1")`、`Delete("k1")`、`Get("k1")`
- THEN 第一次 `Get` SHALL 返回 `"v1"`
- AND 第二次 `Get` SHALL 返回 `"v2"`（更新生效）
- AND 第三次 `Get` SHALL 返回"Key 不存在"（删除生效）

### Scenario: 操作不存在的 Key

- GIVEN 数据库已打开，Key `"nonexist"` 不存在于数据库中
- WHEN 用户依次执行 `Get("nonexist")` 和 `Delete("nonexist")`
- THEN `Get` SHALL 返回"Key 不存在"的指示
- AND `Delete` SHALL 返回"Key 不存在"的指示
- AND 系统 SHALL NOT 产生任何错误或副作用

---

## Requirement 4: 事务支持（ACID）

The system SHALL support single-threaded ACID transactions, allowing users to group multiple operations into an atomic unit that can be committed or rolled back.

### Scenario: 事务正常提交后数据可见

- GIVEN 用户已通过 `Begin()` 开启一个事务
- WHEN 用户在事务内执行 `Put("k1", "v1")` 和 `Put("k2", "v2")`，然后调用 `Commit()`
- THEN 事务提交后，`Get("k1")` SHALL 返回 `"v1"`，`Get("k2")` SHALL 返回 `"v2"`
- AND 事务内读取自身未提交的修改 SHALL 在 `Commit` 前即可见

### Scenario: 事务回滚后修改不可见

- GIVEN Key `"k1"` 已存储，值为 `"original"`
- AND 用户已通过 `Begin()` 开启一个事务，在事务内执行 `Put("k1", "modified")` 和 `Put("k2", "new")`
- WHEN 用户调用 `Rollback()`
- THEN 事务内所有修改 SHALL 被丢弃
- AND `Get("k1")` SHALL 返回 `"original"`（保持事务开始前的状态）
- AND `Get("k2")` SHALL 返回"Key 不存在"

---

## Requirement 5: WAL（Write-Ahead Log）预写日志

The system SHALL implement a Write-Ahead Log (WAL) mechanism. All modifications MUST be written to the WAL log before being applied to the main storage. The system SHALL use the WAL to recover data after an abnormal shutdown.

### Scenario: 崩溃后通过 WAL 恢复已提交事务

- GIVEN 数据库在运行过程中异常崩溃（未正常关闭）
- AND 事务 T1 已提交但其部分修改尚未刷写到主存储文件
- AND 事务 T2 尚未提交
- WHEN 数据库重新启动
- THEN 系统 SHALL 通过 WAL 日志重放（Redo）事务 T1 的所有操作
- AND 系统 SHALL 忽略事务 T2 的未提交操作
- AND 恢复完成后，事务 T1 的所有修改 SHALL 可见，事务 T2 的修改 SHALL 不可见

### Scenario: WAL 日志损坏时部分恢复

- GIVEN WAL 日志文件中存在一条记录，其校验和不匹配
- WHEN 数据库启动并尝试通过 WAL 进行恢复
- THEN 系统 SHALL 重放所有位于损坏记录之前的完整日志记录
- AND 系统 SHALL 停止在损坏记录处，并记录错误信息
- AND 损坏记录之后的内容 SHALL NOT 被重放

---

## 数据需求

### 数据实体

| 实体名称 | 描述 | 核心字段 |
|----------|------|----------|
| Key | 数据库中的键，唯一标识一条记录 | 字节序列，支持任意二进制数据，具有可排序性 |
| Value | 与 Key 关联的值 | 字节序列，支持任意二进制数据 |
| Page | 数据在磁盘上的基本存储单元 | 页号（Page ID）、数据区、校验信息 |
| B+TreeNode | B+树中的节点，存储在 Page 中 | 节点类型（内部节点/叶子节点）、键列表、指针列表、兄弟指针 |
| WALRecord | WAL 日志中的一条记录 | LSN、操作类型、Key、Value、事务 ID |
| Transaction | 事务上下文 | 事务 ID、状态（Active/Committed/Rolledback）、操作列表 |

### 数据流

```
用户操作（Put/Get/Delete/Scan）
        │
        ▼
    事务管理器（Begin/Commit/Rollback）
        │
        ▼
    WAL 日志记录（先写日志）
        │
        ▼
    内存缓冲区（Buffer Pool）
        │
        ▼
    B+树索引（查找/插入/删除）
        │
        ▼
    磁盘文件（Page 持久化）
```

---

## 假设和依赖

### 假设

- **假设 1**: 数据库运行在单线程环境中，不存在并发读写冲突。并发控制不在当前范围内，留作后续迭代。
- **假设 2**: Key 和 Value 均为字节序列（`Vec<u8>`），Key 具有可排序性（通过用户定义的比较器或默认字节序比较）。
- **假设 3**: 文件系统提供原子写入语义（至少在单页写入层面），WAL 机制在此基础上提供更高级别的数据保证。
- **假设 4**: 数据库在同一时刻只被一个进程打开和操作，不涉及多进程共享访问。

### 依赖

- **依赖 1**: Rust 标准库（`std`），包括文件 I/O（`std::fs`）、集合类型（`std::collections`）和系统调用。
- **依赖 2**: 操作系统提供 `fsync`（或等效）系统调用，确保数据写入磁盘而非仅停留在操作系统缓存。
- **依赖 3**: 本地文件系统支持文件的创建、读写、删除和随机访问操作。
