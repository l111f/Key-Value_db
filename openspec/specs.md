# Specs: KV-001 轻量级 Key-Value 存储引擎

> 基于 requirements.md 进行功能需求规格说明。

## 需求列表

### R1: 内存缓冲区与文件持久化

**优先级**: 高
**关联提案**: S-01 内存缓冲区管理, S-02 文件持久化存储

#### 功能描述

系统 SHALL 提供一个存储引擎，通过内存缓冲区（Buffer Pool）管理数据，并以页（Page）为单位将数据持久化到磁盘文件。写入操作先进入内存缓冲区，按照刷写策略（如 LRU 或显式 sync）将脏页写入磁盘。读取操作优先从缓冲区获取数据页，未命中时从磁盘加载。每个数据页 MUST 包含校验信息（如 CRC），用于检测磁盘数据损坏。

#### 场景

**场景 1: 正常写入数据并从磁盘恢复**
- GIVEN 存储引擎已初始化，关联的数据文件已打开
- WHEN 用户调用 `Put("key_001", "value_001")` 并触发缓冲区刷写（sync），随后存储引擎关闭并重新打开
- THEN 系统 SHALL 从磁盘文件中正确加载已持久化的数据页
- AND 调用 `Get("key_001")` SHALL 返回 `"value_001"`

**场景 2: 数据页校验失败时的错误处理**
- GIVEN 磁盘文件中存在一个数据页，其 CRC 校验信息与实际数据不匹配（数据损坏）
- WHEN 存储引擎尝试读取该数据页
- THEN 系统 SHALL 返回明确的错误指示（包含错误类型和页号信息）
- AND 系统 SHALL NOT 使用该损坏页中的任何数据
- AND 系统 SHALL 记录错误日志，包含损坏页的页号

**场景 3: 缓冲区未命中时从磁盘加载**
- GIVEN 存储引擎已初始化，磁盘文件中包含 Key `"alpha"` 的数据，但缓冲区中未缓存该数据页
- WHEN 用户调用 `Get("alpha")`
- THEN 系统 SHALL 从磁盘加载对应的数据页到缓冲区
- AND `Get("alpha")` SHALL 返回正确的值

**场景 4: 缓冲区满时淘汰策略**
- GIVEN 缓冲区已达到容量上限，所有槽位均被占用
- WHEN 用户写入新的数据需要加载新的数据页
- THEN 系统 SHALL 根据 LRU 策略淘汰最近最少使用的缓冲页
- AND 如果被淘汰的页为脏页，系统 SHALL 先将其刷写到磁盘
- AND 新数据页 SHALL 成功加载到缓冲区

**场景 5: 多次写入后数据一致性**
- GIVEN 存储引擎已初始化
- WHEN 用户连续执行 `Put("k1", "v1")`、`Put("k2", "v2")`、`Put("k3", "v3")`，并触发缓冲区刷写，随后关闭并重新打开
- THEN 系统 SHALL 正确恢复所有三条数据
- AND `Get("k1")` SHALL 返回 `"v1"`，`Get("k2")` SHALL 返回 `"v2"`，`Get("k3")` SHALL 返回 `"v3"`

---

### R2: B+树索引

**优先级**: 高
**关联提案**: S-03 B+树索引, S-05 范围扫描

#### 功能描述

系统 SHALL 实现一个 B+树（B+ Tree）索引结构，以有序方式组织所有 Key。B+树 SHALL 支持精确查找（Point Lookup）、范围扫描（Range Scan）以及动态平衡维护（通过节点分裂和合并）。内部节点仅存储键和子节点指针，叶子节点存储完整的键值对，并通过兄弟指针串联以支持高效范围扫描。

#### 场景

**场景 1: 精确查找返回正确值**
- GIVEN B+树中已包含 Key `"a"`, `"c"`, `"e"`, `"g"`, `"i"` 及其对应的 Value
- WHEN 用户对 `"e"` 执行精确查找
- THEN 系统 SHALL 返回 `"e"` 对应的 Value
- AND 查找时间复杂度 SHALL 为 O(log n)

**场景 2: 范围扫描按序返回结果**
- GIVEN B+树中已包含 Key `"a"`, `"c"`, `"e"`, `"g"`, `"i"` 及其对应的 Value
- WHEN 用户执行范围扫描 `Scan("c", "g")`（包含边界）
- THEN 系统 SHALL 按 Key 升序返回 `"c"`, `"e"`, `"g"` 及其对应的 Value
- AND 结果集 SHALL NOT 包含 `"a"` 和 `"i"`

**场景 3: 插入数据触发叶子节点分裂**
- GIVEN B+树的某个叶子节点已满（达到阶数上限）
- WHEN 用户插入一个新的 Key-Value 对，导致该叶子节点溢出
- THEN 系统 SHALL 将该叶子节点分裂为两个节点
- AND 中间键（Median Key）SHALL 被提升到父节点
- AND 分裂后 B+树 SHALL 保持平衡（所有叶子节点处于同一深度）
- AND 所有已存在的 Key 在分裂后 SHALL 仍可通过精确查找正确获取

**场景 4: 连续插入触发根节点分裂（树高度增长）**
- GIVEN B+树当前根节点已满
- WHEN 用户持续插入新的 Key-Value 对，导致根节点分裂
- THEN 系统 SHALL 创建一个新的根节点
- AND B+树的高度 SHALL 增加 1
- AND 新根节点 SHALL 包含分裂产生的中间键和两个子节点指针
- AND 所有已存在的 Key SHALL 仍可通过精确查找正确获取

**场景 5: 删除数据触发节点合并或借用**
- GIVEN B+树中某个叶子节点的 Key 数量降至最小填充阈值以下
- AND 该节点的兄弟节点有足够的空间或 Key
- WHEN 系统执行删除操作后检测到节点下溢（Underflow）
- THEN 系统 SHALL 尝试从兄弟节点借用 Key，或与兄弟节点合并
- AND 合并或借用后 B+树 SHALL 保持平衡
- AND 所有剩余的 Key SHALL 仍可通过精确查找正确获取

**场景 6: 空树的查找与插入**
- GIVEN B+树为空（刚初始化，无任何数据）
- WHEN 用户执行 `Get("any_key")`
- THEN 系统 SHALL 返回"Key 不存在"的指示
- WHEN 用户执行 `Put("first_key", "first_value")`
- THEN 系统 SHALL 创建根叶子节点并存储该键值对
- AND 后续 `Get("first_key")` SHALL 返回 `"first_value"`

---

### R3: 基础 CRUD 操作

**优先级**: 高
**关联提案**: S-04 单键 CRUD

#### 功能描述

系统 SHALL 提供 Key-Value CRUD 操作接口，包括 `Put`、`Get`、`Delete` 和 `Scan`，作为用户与数据库交互的主要接口。所有操作 MUST 通过 B+树索引定位数据。`Put` 插入或更新键值对，`Get` 查找单个键，`Delete` 删除单个键，`Scan` 按范围返回有序键值对。

#### 场景

**场景 1: Put/Get/Delete 完整生命周期**
- GIVEN 数据库已打开且处于初始状态
- WHEN 用户依次执行 `Put("k1", "v1")`、`Get("k1")`、`Put("k1", "v2")`、`Get("k1")`、`Delete("k1")`、`Get("k1")`
- THEN 第一次 `Get("k1")` SHALL 返回 `"v1"`
- AND 第二次 `Get("k1")` SHALL 返回 `"v2"`（更新生效）
- AND 第三次 `Get("k1")` SHALL 返回"Key 不存在"（删除生效）

**场景 2: 操作不存在的 Key**
- GIVEN 数据库已打开，Key `"nonexist"` 不存在于数据库中
- WHEN 用户依次执行 `Get("nonexist")` 和 `Delete("nonexist")`
- THEN `Get("nonexist")` SHALL 返回"Key 不存在"的指示（非错误）
- AND `Delete("nonexist")` SHALL 返回"Key 不存在"的指示（非错误）
- AND 系统 SHALL NOT 产生任何异常或副作用

**场景 3: Put 覆盖已有值**
- GIVEN 数据库中已存在 Key `"config"` 值为 `"old_setting"`
- WHEN 用户执行 `Put("config", "new_setting")`
- THEN `Get("config")` SHALL 返回 `"new_setting"`
- AND 旧值 `"old_setting"` SHALL 被完全替换

**场景 4: Delete 后重新 Put**
- GIVEN 数据库中已存在 Key `"temp"` 值为 `"data"`
- WHEN 用户执行 `Delete("temp")` 后再执行 `Put("temp", "restored")`
- THEN `Get("temp")` SHALL 返回 `"restored"`
- AND 该 Key 的行为 SHALL 与新插入的 Key 一致

**场景 5: 范围扫描 Scan 基本功能**
- GIVEN 数据库中已包含 `Put("k1", "v1")`、`Put("k2", "v2")`、`Put("k3", "v3")`、`Put("k4", "v4")`、`Put("k5", "v5")`
- WHEN 用户执行 `Scan("k2", "k4")`（包含边界）
- THEN 系统 SHALL 返回 `[("k2", "v2"), ("k3", "v3"), ("k4", "v4")]`
- AND 结果按 Key 升序排列

**场景 6: Scan 无匹配结果**
- GIVEN 数据库中已包含 `Put("a", "va")`、`Put("z", "vz")`
- WHEN 用户执行 `Scan("m", "n")`
- THEN 系统 SHALL 返回空结果集
- AND 系统 SHALL NOT 产生任何错误

**场景 7: 空数据库上的操作**
- GIVEN 数据库刚初始化，无任何数据
- WHEN 用户执行 `Get("any")`、`Delete("any")`、`Scan("a", "z")`
- THEN `Get` SHALL 返回"Key 不存在"
- AND `Delete` SHALL 返回"Key 不存在"
- AND `Scan` SHALL 返回空结果集

---

### R4: 事务支持（ACID）

**优先级**: 高
**关联提案**: S-06 事务管理

#### 功能描述

系统 SHALL 支持单线程环境下的 ACID 事务。用户通过 `Begin()` 开启事务，在事务内执行多个操作，最终通过 `Commit()` 提交或 `Rollback()` 回滚。事务 MUST 保证原子性（Atomicity）：要么所有操作生效，要么全部丢弃。事务 MUST 保证一致性（Consistency）：事务前后数据库从一个一致状态转换到另一个一致状态。事务 MUST 保证隔离性（Isolation）：单线程模型下天然保证。事务 MUST 保证持久性（Durability）：提交后的数据 MUST 持久化到磁盘。

#### 场景

**场景 1: 事务正常提交后数据可见**
- GIVEN 用户已通过 `Begin()` 开启一个事务
- WHEN 用户在事务内执行 `Put("k1", "v1")` 和 `Put("k2", "v2")`，然后调用 `Commit()`
- THEN 事务提交后，`Get("k1")` SHALL 返回 `"v1"`，`Get("k2")` SHALL 返回 `"v2"`
- AND 事务内读取自身未提交的修改 SHALL 在 `Commit` 前即可见（本地读一致性）

**场景 2: 事务回滚后修改不可见**
- GIVEN Key `"k1"` 已存储，值为 `"original"`
- AND 用户已通过 `Begin()` 开启一个事务，在事务内执行 `Put("k1", "modified")` 和 `Put("k2", "new")`
- WHEN 用户调用 `Rollback()`
- THEN 事务内所有修改 SHALL 被丢弃
- AND `Get("k1")` SHALL 返回 `"original"`（保持事务开始前的状态）
- AND `Get("k2")` SHALL 返回"Key 不存在"

**场景 3: 事务内多操作原子性**
- GIVEN 用户已通过 `Begin()` 开启一个事务
- WHEN 用户在事务内执行 `Put("a", "1")`、`Delete("b")`、`Put("c", "3")`，然后调用 `Commit()`
- THEN 所有三个操作 SHALL 作为一个原子单元全部生效
- AND `Get("a")` SHALL 返回 `"1"`，`Get("c")` SHALL 返回 `"3"`

**场景 4: 嵌套事务不支持时的处理**
- GIVEN 用户已通过 `Begin()` 开启了一个事务，事务处于 Active 状态
- WHEN 用户在未提交或回滚当前事务的情况下再次调用 `Begin()`
- THEN 系统 SHALL 返回错误提示，表明嵌套事务不被支持
- AND 原事务 SHALL 保持 Active 状态，不受影响

**场景 5: 未开启事务时直接调用 Commit/Rollback**
- GIVEN 当前没有活跃的事务
- WHEN 用户直接调用 `Commit()` 或 `Rollback()`
- THEN 系统 SHALL 返回明确的错误指示（如"没有活跃事务"）
- AND 系统 SHALL NOT 产生任何副作用

**场景 6: 事务提交后持久性保证**
- GIVEN 用户已在事务内执行 `Put("persist_key", "persist_value")` 并调用 `Commit()` 成功
- WHEN 存储引擎关闭后重新打开
- THEN `Get("persist_key")` SHALL 返回 `"persist_value"`
- AND 数据 SHALL 已被持久化到磁盘

---

### R5: WAL 预写日志

**优先级**: 高
**关联提案**: S-07 WAL 预写日志, S-08 崩溃恢复

#### 功能描述

系统 SHALL 实现预写日志（Write-Ahead Log, WAL）机制。所有数据修改操作 MUST 在应用到主存储之前先写入 WAL 日志。WAL 日志以追加写入（Append-Only）方式记录每条操作，每条记录包含日志序列号（LSN）、操作类型、Key、Value 和事务 ID。系统 SHALL 利用 WAL 日志在异常崩溃后重放（Redo）已提交事务的操作，恢复数据一致性。每条 WAL 记录 MUST 包含校验和信息，用于检测日志损坏。

#### 场景

**场景 1: 崩溃后通过 WAL 恢复已提交事务**
- GIVEN 数据库在运行过程中异常崩溃（未正常关闭）
- AND 事务 T1 已 `Commit()` 但其部分修改尚未刷写到主存储文件
- AND 事务 T2 尚未 `Commit()`（状态为 Active）
- WHEN 数据库重新启动并执行恢复流程
- THEN 系统 SHALL 通过 WAL 日志重放（Redo）事务 T1 的所有操作
- AND 系统 SHALL 忽略事务 T2 的未提交操作（不重放）
- AND 恢复完成后，事务 T1 的所有修改 SHALL 可通过 `Get` 正确获取
- AND 事务 T2 的修改 SHALL NOT 可见

**场景 2: WAL 日志损坏时部分恢复**
- GIVEN WAL 日志文件中第 N 条记录的校验和不匹配（记录损坏）
- AND 第 1 至 N-1 条记录均完整且校验通过
- WHEN 数据库启动并尝试通过 WAL 进行恢复
- THEN 系统 SHALL 重放第 1 至 N-1 条完整的日志记录
- AND 系统 SHALL 停止在损坏记录（第 N 条）处，并记录错误信息
- AND 第 N 条之后的所有日志记录 SHALL NOT 被重放
- AND 系统 SHALL 在日志中标记 WAL 文件需要修复或截断

**场景 3: 正常关闭时 WAL 日志清理**
- GIVEN 数据库正常关闭，所有脏页已刷写到磁盘
- AND WAL 日志中所有记录对应的修改均已持久化
- WHEN 数据库执行关闭流程
- THEN 系统 MAY 清空或标记 WAL 日志文件为已归档
- AND 下次启动时 SHALL NOT 需要重放已归档的 WAL 日志

**场景 4: WAL 写入顺序保证**
- GIVEN 用户在事务 T1 中依次执行 `Put("k1", "v1")`、`Put("k2", "v2")`
- WHEN 系统 WAL 日志模块记录这些操作
- THEN WAL 日志中 `Put("k1", "v1")` 的记录 SHALL 出现在 `Put("k2", "v2")` 之前
- AND 事务 T1 的 `Commit` 标记 SHALL 出现在所有操作记录之后
- AND 系统 SHALL 保证 WAL 记录的顺序与操作执行顺序一致

**场景 5: 空崩溃恢复（无需重放）**
- GIVEN 数据库上次正常关闭，WAL 日志为空或已被归档
- WHEN 数据库重新启动
- THEN 系统 SHALL 检测到无需恢复，直接进入正常服务状态
- AND 系统 SHALL NOT 执行任何 WAL 重放操作

---

## 数据规格

### 数据实体

| 实体名称 | 描述 | 核心字段 | 约束 |
|----------|------|----------|------|
| Key | 数据库中的键，唯一标识一条记录 | 字节序列（`Vec<u8>`） | 支持任意二进制数据；具有可排序性（默认字节序比较）；MUST 在数据库内唯一 |
| Value | 与 Key 关联的值 | 字节序列（`Vec<u8>`） | 支持任意二进制数据；长度无上限约束（受磁盘空间限制） |
| Page | 数据在磁盘上的基本存储单元 | 页号（Page ID, `u32`）、数据区（`[u8; PAGE_SIZE]`）、校验信息（CRC32）、脏标记（Dirty Flag） | 页大小 SHALL 为固定值（如 4KB）；每个 Page 通过 Page ID 唯一标识 |
| B+TreeNode | B+树中的节点，存储在 Page 中 | 节点类型（Internal / Leaf）、键列表（`Vec<Key>`）、指针列表（子节点 Page ID 或兄弟指针）、Value 列表（仅叶子节点） | 内部节点仅存 Key 和子节点指针；叶子节点存 Key-Value 对和兄弟指针；节点阶数 SHALL 可配置 |
| WALRecord | WAL 日志中的一条记录 | LSN（`u64`，单调递增）、操作类型（Put / Delete / Commit / Rollback）、Key、Value、事务 ID（`TxnId`）、校验和（CRC32） | LSN MUST 全局单调递增；每条记录 MUST 包含校验和 |
| Transaction | 事务上下文 | 事务 ID（`TxnId`）、状态（Active / Committed / Rolledback）、操作列表（`Vec<WALRecord>`）、开始时间戳 | 同一时刻 SHALL 最多存在一个 Active 事务 |

### 数据流

```
用户操作（Put / Get / Delete / Scan）
        │
        ▼
    事务管理器（Begin / Commit / Rollback）
        │
        ▼
    WAL 日志记录（先写日志，Append-Only）
        │
        ▼
    内存缓冲区（Buffer Pool，LRU 淘汰策略）
        │
        ▼
    B+树索引（查找 / 插入 / 删除 / 范围扫描）
        │
        ▼
    磁盘文件（Page 持久化，4KB 页对齐）
```

**写入路径**：用户调用 `Put` → 事务管理器记录操作 → WAL 日志追加写入（`fsync`） → 内存缓冲区标记脏页 → B+树索引更新 → 按策略刷写到磁盘。

**读取路径**：用户调用 `Get` → B+树索引查找 → 内存缓冲区获取数据页（未命中则从磁盘加载） → 返回 Value。

**崩溃恢复路径**：数据库启动 → 检测 WAL 日志 → 重放已提交事务的记录 → 忽略未提交事务 → 恢复完成，进入正常服务。

## 假设与约束

### 假设

- **假设 1（单线程）**：数据库运行在单线程环境中，不存在并发读写冲突。并发控制（如 MVCC、锁机制）不在当前范围内，留作后续迭代。
- **假设 2（Key/Value 格式）**：Key 和 Value 均为字节序列（`Vec<u8>`），Key 具有可排序性（通过用户定义的比较器或默认字节序比较）。
- **假设 3（文件系统原子写入）**：文件系统提供原子写入语义（至少在单页写入层面），WAL 机制在此基础上提供更高级别的数据保证。
- **假设 4（单进程访问）**：数据库在同一时刻只被一个进程打开和操作，不涉及多进程共享访问。

### 约束

- **约束 1**：项目 MUST 使用 Rust 语言实现，构建工具为 Cargo。
- **约束 2**：存储介质 MUST 为本地文件系统，使用标准文件 I/O（`std::fs`）进行持久化。
- **约束 3**：不引入外部数据库依赖，所有存储逻辑 MUST 自行实现。
- **约束 4**：WAL 的 `fsync` 调用 MUST 在事务提交时执行，以保证持久性。
- **约束 5**：Page 大小 SHALL 为 4KB（4096 字节），与操作系统页大小对齐。

### 依赖

- **依赖 1**：Rust 标准库（`std`），包括文件 I/O（`std::fs`）、集合类型（`std::collections`）和系统调用。
- **依赖 2**：操作系统提供 `fsync`（或等效）系统调用，确保数据写入磁盘而非仅停留在操作系统缓存。
- **依赖 3**：本地文件系统支持文件的创建、读写、删除和随机访问操作。
