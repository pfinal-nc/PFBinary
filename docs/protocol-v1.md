# PFBinary Protocol Specification v1.0

**文档版本：** 1.0  
**状态：** 定稿（实现前规范）  
**兼容目标：** PHP 5.6+（V1 仅编码 NULL / BOOL / INT / DOUBLE / STRING / ARRAY）  
**对照基线：** igbinary wire v2、phpser wire v1/v2

---

## 1. 设计目标

| 目标 | 说明 |
|------|------|
| Decode 优先 | 格式声明 count / length，Decoder 可一次预分配 |
| 紧凑 | 相对 `serialize()` 典型体积降低 ≥ 30% |
| 分支少 | 单字节 tag + VarInt，避免 igbinary 式定宽 tag 族爆炸 |
| 可演进 | Header Version + 预留 tag 号段，V2 不破坏 V1 Decoder |
| 安全默认 | 未知 tag / 截断 / 溢出一律失败，不产生部分 zval |

非目标（V1）：Object、Reference、压缩、igbinary 线格式兼容、zero-copy。

---

## 2. 整体布局

```text
┌──────────────────┐
│ Magic            │ 4 bytes  "PFBN" (0x50 0x46 0x42 0x4E)
├──────────────────┤
│ Version          │ 1 byte   当前 = 0x01
├──────────────────┤
│ Flags            │ 1 byte   见 §3
├──────────────────┤
│ Checksum         │ 0 或 4 bytes（Flags bit0 控制）
├──────────────────┤
│ Payload          │ variable 单个顶层 Value（§5）
└──────────────────┘
```

- 端序：多字节定宽字段一律 **little-endian**（仅 `DOUBLE`、可选 `Checksum`）。
- Payload 恰好包含 **一个** 顶层 Value；其后不得有多余字节（严格模式下多余字节视为错误）。
- Checksum（若启用）覆盖范围：从 Payload 起始到 Payload 结束（不含 Header / Checksum 自身）。

---

## 3. Flags

| Bit | 名称 | 含义 |
|-----|------|------|
| 0 | `FLAG_CHECKSUM` | 1 = Header 后跟 4 字节校验和 |
| 1 | `FLAG_STRDEDUP` | 1 = 启用字符串去重（允许 `STR_REF`）；0 = 每个字符串一律 `STR_VARINT` / `STR_EMPTY` |
| 2–7 | 预留 | 必须写 0；Decoder 遇到未知置位 → 错误 `E_UNKNOWN_FLAGS` |

**V1 推荐默认 Flags：** `0x02`（启用去重，关闭校验）。  
生产环境若担心 Redis 损坏，可开启 `0x03`（去重 + 校验）。

---

## 4. Checksum

| 项目 | 规范 |
|------|------|
| 算法 | XXH3-64 的低 32 位（`xxh3_64(payload) & 0xFFFF_FFFF`），小端写入 |
| 位置 | Flags 之后、Payload 之前，固定 4 字节 |
| 默认 | **关闭**（Encode 成本与兼容性优先） |
| 失败 | 校验不匹配 → `E_CHECKSUM`，不进入 Value 解析 |

选择 XXH3 而非 CRC32：吞吐更高，适合热点 decode 路径上的可选完整性检查。  
V1 实现可依赖 `xxhash-rust` 或等价 C 实现；算法标识不写入 Flags（V1 固定 XXH3-32）。

---

## 5. Value 与 Tag 表

每个 Value 以 **1 字节 Tag** 开头，后跟 Tag 定义的载荷。

### 5.1 V1 已定义 Tag

| Tag | 助记名 | 载荷 | PHP 类型 |
|-----|--------|------|----------|
| `0x00` | `NULL` | （无） | `null` |
| `0x01` | `FALSE` | （无） | `false` |
| `0x02` | `TRUE` | （无） | `true` |
| `0x03` | `VARINT` | `uvarint(zigzag(i64))` | `int` / `long` |
| `0x04` | `DOUBLE` | 8 bytes IEEE-754 LE | `float` / `double` |
| `0x05` | `STR_EMPTY` | （无） | `""` |
| `0x06` | `STR_VARINT` | `uvarint(len)` + `len` bytes（原始字节，非 UTF-8 强制） | `string` |
| `0x07` | `STR_REF` | `uvarint(string_id)` | `string`（查表） |
| `0x08` | `ARRAY_PACKED` | `uvarint(count)` + `count` × Value | packed array |
| `0x09` | `ARRAY_HASH` | `uvarint(count)` + `count` × (Key, Value) | associative / mixed array |
| `0x0A` | `PACKED_LONGS` | `uvarint(count)` + `count` × zigzag-varint（**无** per-element Tag） | 全为 `int` 的 packed list |
| `0x0B` | `ARRAY_ROWS` | `uvarint(nrows)` + `uvarint(ncols)` + `ncols`×Key + `nrows`×(`ncols`×Value) | 同构哈希行列表（键表只写一次） |
| `0x0C` | `ARRAY_REPEAT` | `uvarint(count)` + Value | packed list，`count` 个相同元素（只写一次） |

### 5.2 Key（仅用于 `ARRAY_HASH`）

Key 不是独立顶层 Value，而是受限的键编码：

| 形式 | 编码 |
|------|------|
| 整数键 | `0x03 VARINT` + zigzag varint（与普通 INT 相同） |
| 字符串键 | `0x05` / `0x06` / `0x07`（与普通 STRING 相同，共享 String Table） |

禁止 Key 为 NULL / BOOL / DOUBLE / ARRAY。Decoder 遇到非法 Key Tag → `E_BAD_KEY`。

### 5.3 预留号段

| 范围 | 用途 |
|------|------|
| `0x0D`–`0x3F` | V1.x 扩展 |
| `0x40`–`0x7F` | V2：OBJECT / REF / CUSTOM_SERIALIZE 等 |
| `0x80`–`0xFF` | 远期扩展 / 实验 |

V1 Decoder 遇到 `≥ 0x0D` 的 Tag → `E_UNKNOWN_TAG`（不猜测、不跳过）。

---

## 6. VarInt 规格

### 6.1 无符号 VarInt（`uvarint`）

采用 **LEB128**：

- 每字节低 7 位为数据，最高位为延续位（1 = 后续还有字节）。
- 小端分组（低位字节在前）。
- 最大长度：**10 字节**（覆盖完整 `u64`）。超过 10 字节或第 10 字节仍有延续位 → `E_VARINT`。
- 解码后若用于长度 / count，必须再与剩余字节数、`max_size` 交叉校验。

### 6.2 ZigZag（有符号整数）

对 `i64` 值 `n`：

```text
zigzag(n) = (n << 1) ^ (n >> 63)   // 算术右移
```

解码：

```text
n = (u >> 1) ^ (-(u & 1))   // 转为 i64
```

| 原值 | ZigZag | 典型 uvarint 字节数 |
|------|--------|---------------------|
| 0 | 0 | 1 |
| -1 | 1 | 1 |
| 1 | 2 | 1 |
| 63 | 126 | 1 |
| -64 | 127 | 1 |
| 64 | 128 | 2 |
| 10001 | 20002 | 3 |

**决策 D1：** 用单一 `VARINT` Tag 替代 igbinary 的 `long8p/8n/16p/16n/32p/32n/64p/64n` 八个定宽 Tag，减少 decode 分支；代价是热路径上多一次变长解析，需由 benchmark 验证。

### 6.3 PHP 5.6 整数范围

- PHP 5.6 在 64 位 Linux 上 `long` 为 64 位；32 位构建上超出 `INT32` 范围的整数在原生 `serialize` 中会变成 `double`。
- PFBinary V1：**平台无关地按 i64 ZigZag 编码**；Decode 到 PHP 5.6 32 位时，若超出 `LONG_MAX`/`LONG_MIN`，转为 `double`（与 PHP 自身行为对齐），并记入实现说明。

---

## 7. String Table（内联首次定义）

### 7.1 模型

**不采用** README 草稿中的「前置独立 String Table 区块」。

采用与 igbinary 相同的语义模型，但 id 使用变长编码：

```text
首次出现某字符串 S（len > 0）
  → 写出 STR_VARINT(len, bytes)
  → 将该字符串分配下一个 string_id（从 0 起递增）
  → 同时记入 Encoder / Decoder 侧表

再次出现 S
  → 写出 STR_REF(string_id)

空串
  → 始终 STR_EMPTY（不进入表，不占用 id）
```

### 7.2 为何放弃前置独立区块

| 方案 | Encode | Decode | 问题 |
|------|--------|--------|------|
| 前置独立 String Table | 需两遍扫描或缓冲整树 | 可先建表再解 Value | Encode 成本高，违背「允许略慢但仍可控」；大对象峰值内存高 |
| 内联首次定义 + REF | 单遍 | 边解边建表 | 与 igbinary/phpser 经验一致；id 用 varint 比 igbinary 定宽 id 更省 |

**决策 D2：** 内联首次定义 + `STR_REF` + varint id。  
`FLAG_STRDEDUP=0` 时 Encoder 禁止发出 `STR_REF`，每次都发 `STR_VARINT`（便于调试与 A/B）。

### 7.3 作用域

- String Table 作用域 = **整份 Payload（一次 encode 调用）**。
- 数组 Key 与 Value 中的字符串 **共享同一张表**。
- V1 不把「仅出现一次的短串」特殊化为 `STR_INLINE` 跳过入表（phpser 的 `0x0c STR_INLINE`）；V1.1 可评估。

### 7.4 安全

- `string_id` 必须 `<` 当前表长度，否则 `E_BAD_STRREF`。
- `STR_VARINT` 的 `len` 必须 `≤` 剩余字节，且累计字符串字节计入 `max_size`。

---

## 8. Array Layout

### 8.1 Packed 判定（Encode）

满足以下全部条件时使用 packed 族（`PACKED_LONGS` 或 `ARRAY_PACKED`）：

1. 元素个数为 `n`（`n ≥ 0`）。
2. 键恰好为整数 `0, 1, …, n-1`（顺序遍历 HashTable 时的插入序即该序）。
3. 无「洞」（PHP 中删除中间元素后留下的稀疏数组不算 packed）。

在此基础上：

- 若 `n ≥ 1` 且每个元素均为整数（PHP `IS_LONG` / 协议 `VARINT`）→ **必须** 使用 `PACKED_LONGS`。
- 否则 → `ARRAY_PACKED`（元素各自带 Tag）。

否则使用 `ARRAY_HASH`，并显式写出每个 Key。

空数组：`ARRAY_PACKED` + `count=0`（推荐）或 `ARRAY_HASH` + `count=0`（等价）；Encoder **应**选 `ARRAY_PACKED`（空数组不用 `PACKED_LONGS`）。

### 8.2 Decode 行为

| Tag | 行为 |
|-----|------|
| `ARRAY_PACKED` | 读取 `count` → 预分配容量 ≥ `count` 的 PHP 数组 → 按序 `append` / next-index insert `count` 个 Value |
| `ARRAY_HASH` | 读取 `count` → 预分配容量 ≥ `count` → 循环 `count` 次：读 Key、读 Value、插入 |
| `PACKED_LONGS` | 读取 `count` → 预分配 packed → 读 `count` 个 zigzag-varint → 批量 `append` 为整数 |
| `ARRAY_ROWS` | 读 `nrows`/`ncols` → 读一次 Key 表 → 预分配 packed 外层 → 每行建 hash 并按列插入 Value |
| `ARRAY_REPEAT` | 读 `count` → 解码一次 Value → `ZVAL_COPY` / 等价复制 `count` 次追加 |

### 8.3 同构行判定（Encode → `ARRAY_ROWS` / `ARRAY_REPEAT`）

优先级（packed list）：

1. 全整数 → `PACKED_LONGS`
2. `count ≥ 2` 且元素两两 `===` 相同 → `ARRAY_REPEAT`
3. `count ≥ 2` 且均为同构 hash 行 → `ARRAY_ROWS`
4. 否则 → `ARRAY_PACKED`

`ARRAY_ROWS` 条件：各行键序列完全一致（类型、顺序、值相同；字符串键按字节相等）；`ncols ≥ 1`。

空外层或单行：仍用 `ARRAY_PACKED` / 普通 hash。

目标：**避免 HashTable 多次扩容**（对齐 phpser「pre-sized HT」思路）。

### 8.4 嵌套深度

每进入一个 ARRAY（含 `ARRAY_ROWS` 的外层与每一行），`depth += 1`；离开时减一。  
`depth > max_depth` → `E_MAX_DEPTH`。默认 `max_depth = 100`。

---

## 9. 版本演进

### 9.1 Version 字节

| Version | 含义 |
|---------|------|
| `0x01` | 本规范 |
| `0x02+` | 未来；V1-only Decoder 应返回 `E_UNSUPPORTED_VERSION` |

### 9.2 兼容规则

1. **同主版本内** 仅可增加 Flags 预留位的**可选**语义；旧 Decoder 遇未知 Flags 位必须失败（安全优先，不做静默忽略）。
2. **新 Tag** 只能占用预留号段；旧 Decoder 遇未知 Tag 失败。
3. **不得** 复用已定义 Tag 改变载荷布局。
4. 破坏性变更必须升 `Version`，并在实现中提供 `decode_v1` / `decode_v2` 分发。

### 9.3 Magic 校验

Magic ≠ `PFBN` → `E_BAD_MAGIC`（可用于灰度迁移时快速识别「非本格式，回退 igbinary」）。

---

## 10. Malformed 与错误语义

### 10.1 原则

> **失败必须原子：要么得到完整顶层 zval，要么得到错误码；禁止返回半成品结构。**

实现上：Decode 过程中分配的临时 zval / 数组在错误路径上必须全部 `zval_ptr_dtor`。

### 10.2 错误码（协议层语义，数值见架构文档 ABI）

| 符号 | 触发条件 |
|------|----------|
| `E_BAD_MAGIC` | Magic 不匹配 |
| `E_UNSUPPORTED_VERSION` | Version ≠ 1（对本 Decoder） |
| `E_UNKNOWN_FLAGS` | 预留 Flags 位置位 |
| `E_CHECKSUM` | 校验失败 |
| `E_TRUNCATED` | 需要更多字节但输入结束 |
| `E_TRAILING` | Payload 解析完成后仍有剩余字节（严格模式） |
| `E_UNKNOWN_TAG` | Tag 未定义或不属于当前 Version |
| `E_BAD_KEY` | HASH 数组出现非法 Key Tag |
| `E_VARINT` | VarInt 过长 / 溢出 |
| `E_BAD_STRREF` | string_id 越界 |
| `E_MAX_DEPTH` | 嵌套超过 `max_depth` |
| `E_MAX_SIZE` | 声明长度或累计分配超过 `max_size` |
| `E_OVERFLOW` | count/len 与剩余字节不一致，或算术溢出 |

### 10.3 长度与 DoS 防护（借鉴 phpser）

在分配任何大缓冲 / 大数组之前：

```text
if declared_count > remaining_bytes:          reject  // 最坏每元素至少 1 字节 Tag
if declared_str_len > remaining_bytes:        reject
if estimated_alloc > max_size:                reject
```

禁止「先按 wire 上的超大 count `malloc`，再发现数据不够」的路径。

---

## 11. 对照与远期（非 V1 必做）

### 11.1 与 igbinary 的差异

| 点 | igbinary | PFBinary v1 |
|----|----------|-------------|
| 整数 | 8 个定宽 long Tag | 单一 `VARINT` + ZigZag |
| 字符串 id | `id8/16/32` 定宽 | `STR_REF` + uvarint |
| 数组 | 统一 array + 显式写出所有键（含 0..n-1） | `ARRAY_PACKED` / `PACKED_LONGS` 省略键 |
| 字符串表 | 内联首次定义 | 同左 |
| Object/Ref | 有 | V1 无（号段预留） |

### 11.2 从 phpser 可借鉴、后续可引入的 Tag

| 想法 | 收益 | 态度 |
|------|------|------|
| `PACKED_LONGS` / 同质整数跑（无 per-element Tag） | packed 整数 decode / 体积 | **已实现（`0x0A`）** |
| `ARRAY_ROWS`（同构哈希行，键表一次） | `repeated_keys` / 结果集体积与 decode | **已实现（`0x0B`）** |
| 前置 / 字典 intern + pointer 相等 | encode 快 | Rust 侧可用；线格式仍内联定义 |
| 列式 `TABLE` | DB 结果集进一步压缩 | V2+ |
| HMAC 签名帧 | 不可信存储 | V1 仅可选 XXH3；签名另议 |

V1 已吸收的 phpser 原则：

1. Decode 前已知 `count` → 预分配。  
2. 声明长度 vs `remaining_bytes` 硬校验。  
3. Decode 优先于 Encode 花样。

---

## 12. 手工演算样例（协议无歧义验证）

约定：下列样例均使用 `Flags = 0x02`（去重开、校验关），故 Header 恒为：

```text
50 46 42 4E  01  02
"P  F  B  N" Ver Flags
```

### 12.1 样例 A — README §9 关联数组

```php
[
  'id' => 10001,
  'name' => 'Tom',
  'price' => 99.99,
  'active' => true,
  'extra' => null,
]
```

逻辑结构：

```text
ARRAY_HASH count=5
  STR_VARINT "id"     → id0    + VARINT(10001)
  STR_VARINT "name"   → id1    + STR_VARINT "Tom" → id2
  STR_VARINT "price"  → id3    + DOUBLE(99.99)
  STR_VARINT "active" → id4    + TRUE
  STR_VARINT "extra"  → id5    + NULL
```

关键字段字节（Payload 内）：

| 片段 | 十六进制（示意） | 说明 |
|------|------------------|------|
| Tag + count | `09` `05` | ARRAY_HASH，5 对 |
| key `id` | `06` `02` `69 64` | STR_VARINT len=2 `"id"` → 表 id=0 |
| val 10001 | `03` `A2 9C 01` | VARINT zigzag(10001)=20002 → LEB128 `A2 9C 01` |
| key `name` | `06` `04` `6E 61 6D 65` | → 表 id=1 |
| val `Tom` | `06` `03` `54 6F 6D` | → 表 id=2 |
| key `price` | `06` `05` `70 72 69 63 65` | → 表 id=3 |
| val 99.99 | `04` `8F C2 F5 28 5C FF 58 40` | DOUBLE（IEEE-754 LE） |
| key `active` | `06` `06` `61 63 74 69 76 65` | → 表 id=4 |
| val true | `02` | TRUE |
| key `extra` | `06` `05` `65 78 74 72 61` | → 表 id=5 |
| val null | `00` | NULL |

相对 `serialize()`（文本 `a:5:{s:2:"id";i:10001;...}`）体积明显更小；相对 igbinary，省去每个整数键的定宽 Tag 族，且本例无整数键。

### 12.2 样例 B — 重复字符串（README §11）

```php
[
  ['id'=>1, 'status'=>'active'],
  ['id'=>2, 'status'=>'active'],
  ['id'=>3, 'status'=>'active'],
]
```

逻辑结构（外层 packed，内层 hash）：

```text
ARRAY_PACKED count=3
  ARRAY_HASH count=2
    STR_VARINT "id"     →0   + VARINT(1)
    STR_VARINT "status" →1   + STR_VARINT "active" →2
  ARRAY_HASH count=2
    STR_REF 0                + VARINT(2)
    STR_REF 1                + STR_REF 2
  ARRAY_HASH count=2
    STR_REF 0                + VARINT(3)
    STR_REF 1                + STR_REF 2
```

第二、三行不再重复写出 `"id"` / `"status"` / `"active"` 字节，仅 `07` + 单字节级 id。  
这是相对 `serialize()` 达成 ≥30% 缩减的主要场景之一。

### 12.3 样例 C — Packed 数值数组（`PACKED_LONGS`）

```php
[1, 2, 3, 4]
```

```text
Header: 50 46 42 4E 01 02
Payload:
  0A          PACKED_LONGS
  04          count = 4
  02          zigzag(1)=2
  04          zigzag(2)=4
  06          zigzag(3)=6
  08          zigzag(4)=8
```

完整 hex：

```text
50 46 42 4E 01 02 0A 04 02 04 06 08
```

对比 igbinary：igbinary 对 packed 数组仍会写出键 `0,1,2,3` 各一组 long Tag，PFBinary 完全省略键，且整数元素无 per-element type Tag。

### 12.4 Roundtrip 不变量

对 V1 支持的类型：

```text
type_php(decode(encode(x))) == type_php(x)
value 语义相等（=== 对标量；数组键值递归相等）
```

浮点：按 IEEE-754 位型往返（`NAN` 载荷实现可选规范化，V1 建议保留静默 NaN 比特或统一为规范 NaN，并在实现与测试中固定一种）。

---

## 13. 规范检查清单（实现前）

- [ ] Tag 表与 §5.1 一字不差  
- [ ] ZigZag + LEB128 与 §6 向量一致（至少覆盖 0/-1/1/63/-64/64/10001）  
- [ ] String id 分配顺序与 §7 一致（空串不入表）  
- [ ] Packed 判定与 §8.1 一致  
- [ ] 所有错误路径不泄漏部分 zval  
- [ ] `max_depth` / `max_size` / remaining-bytes 守卫在分配之前  

---

## 14. 变更记录

| 版本 | 日期 | 说明 |
|------|------|------|
| 1.0 | 2026-09-10 | 首版定稿：内联 String Table、VarInt、PACKED/HASH、可选 XXH3-32 |
| 1.1 | 2026-09-10 | 实现 `PACKED_LONGS`（`0x0A`）；未知 Tag 起点改为 `≥ 0x0B` |
| 1.2 | 2026-09-10 | 实现 `ARRAY_ROWS`（`0x0B`）；未知 Tag 起点改为 `≥ 0x0C` |
| 1.3 | 2026-09-10 | 实现 `ARRAY_REPEAT`（`0x0C`）；未知 Tag 起点改为 `≥ 0x0D` |
