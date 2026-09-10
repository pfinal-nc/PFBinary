# PFBinary

可以。下面我不把它写成“Rust 重写 igbinary”的简单技术方案，而是按一个**可以真正立项、开发、Benchmark、发布的基础设施项目**来写。

项目暂定名：**PFSerialize**。如果以后你想做成开源项目，可以再换正式名称。

---

# PFSerialize

## Rust-powered PHP Serialization Engine

### 面向 PHP 老系统 / Redis 缓存的高性能序列化引擎需求规格说明书

**文档版本：** v1.0
**目标语言：** Rust + C/PHP Extension
**首要兼容版本：** PHP 5.6
**主要应用场景：** PHP + Redis / Memcached / Session / APCu
**核心目标：** 降低缓存数据体积、网络 IO、序列化/反序列化 CPU 成本，并为 PHP 老系统提供低侵入式替换方案。

---

# 1. 项目背景

大量传统 PHP 系统仍然运行在：

* PHP 5.6
* PHP 7.x
* 老版本 Laravel
* ThinkPHP
* WordPress
* 老电商系统
* CRM / ERP
* CMS
* 自研 PHP 框架

这些系统普遍存在一个问题：

```text
PHP Array / Object
       ↓
serialize()
       ↓
String
       ↓
Redis
```

读取：

```text
Redis
 ↓
String
 ↓
unserialize()
 ↓
PHP Array / Object
```

随着 Redis 数据规模增长，序列化格式会直接影响：

1. Redis 内存占用
2. Redis 网络流量
3. PHP CPU
4. PHP 请求延迟
5. Redis RDB/AOF 体积
6. 主从复制流量
7. 缓存命中后的反序列化成本

现有 igbinary 已经解决了大量问题。它通过二进制格式、重复字符串复用和更紧凑的整数编码，相比 PHP 原生 `serialize()` 通常可以明显降低存储需求；官方说明典型存储降低约 50%，具体取决于数据。其 2.x 分支还支持 PHP 5.2–5.6。([GitHub][1])

因此，本项目**不是简单复制 igbinary**，而是进一步针对：

> **PHP 老系统 + Redis + 读多写少缓存**

进行专门优化。

---

# 2. 项目定位

项目定位：

> **一个 Rust 驱动的 PHP 高性能二进制序列化引擎。**

核心不是：

```text
Rust 版 igbinary
```

而是：

```text
PHP Cache Serialization Engine
```

重点优化：

```text
             Redis Cache
                  │
       ┌──────────┼──────────┐
       │          │          │
     内存        网络       CPU
       │          │          │
       └──────────┼──────────┘
                  │
            PFSerialize
```

---

# 3. 核心目标

## 3.1 存储目标

相比 PHP `serialize()`：

### P0

目标：

```text
平均序列化数据体积降低 ≥ 30%
```

### P1

目标：

```text
典型复杂数组降低 ≥ 40%
```

### P2

优秀目标：

```text
复杂重复结构降低 ≥ 50%
```

这里不能承诺固定比例，因为实际压缩率高度依赖数据结构。

---

# 4. 性能目标

## 4.1 Encode

目标：

```text
PFSerialize encode
≤ serialize() × 1.2
```

在开启字符串去重的情况下，允许 encode 略慢。

这是合理的，因为 igbinary 官方也明确说明，重复字符串跟踪会增加序列化时的 Hash Table 开销，而典型缓存场景往往是“serialize 少、unserialize 多”。([GitHub][2])

---

## 4.2 Decode

这是整个项目的核心。

目标：

```text
PFSerialize decode
≥ igbinary
```

最终目标：

```text
典型 Redis Cache Workload

PFSerialize decode
比 igbinary 快 20%+
```

优秀目标：

```text
快 30% ~ 50%
```

但这个目标必须通过真实 benchmark 验证，而不是预先假设。

---

# 5. 最核心的设计原则

## 原则一：Decode 优先

典型 Redis：

```text
SET
 ↓
serialize
 ↓
Redis

GET
 ↓
unserialize
 ↓
PHP
```

很多系统：

```text
写 1 次
读 100 次
```

所以：

```text
Encode：允许稍微慢
Decode：必须极快
```

这是整个项目和传统 serializer 最大的设计区别之一。

---

# 6. PHP 支持范围

## V1

必须支持：

```text
PHP 5.6
Linux x86_64
Linux ARM64
```

PHP 5.6 是第一目标。

---

## V2

增加：

```text
PHP 7.0
PHP 7.4
PHP 8.x
```

但不能为了现代 PHP 破坏 PHP 5.6。

PHP 扩展本身需要直接对接 Zend API；PHP 扩展开发涉及 zval、数组、引用、Copy-on-Write、内存管理等底层机制，因此 PHP 适配层应该与 Rust 核心严格分离。([Zend][3])

---

# 7. 总体架构

```text
                    PHP Application
                          │
                          │
                pfserialize_encode()
                          │
                          ▼
              ┌────────────────────┐
              │ PHP Extension      │
              │                    │
              │ Zend API Adapter   │
              └─────────┬──────────┘
                        │
                        │ C ABI
                        ▼
              ┌────────────────────┐
              │ Rust Core          │
              │                    │
              │ Encoder            │
              │ Decoder            │
              │ String Table       │
              │ Type Codec         │
              │ Reference Tracker  │
              │ Object Codec       │
              └─────────┬──────────┘
                        │
                        ▼
                  PFBinary
                        │
                        ▼
                      Redis
```

---

# 8. 项目代码结构

建议：

```text
pfserialize/
│
├── Cargo.toml
├── README.md
├── LICENSE
│
├── crates/
│   │
│   ├── pf-core/
│   │   ├── encoder.rs
│   │   ├── decoder.rs
│   │   ├── value.rs
│   │   ├── string_table.rs
│   │   ├── references.rs
│   │   └── error.rs
│   │
│   ├── pf-format/
│   │   ├── header.rs
│   │   ├── tags.rs
│   │   └── version.rs
│   │
│   ├── pf-bench/
│   │   ├── datasets/
│   │   └── benchmark.rs
│   │
│   └── pf-cli/
│       └── main.rs
│
├── php/
│   ├── php_pfserialize.h
│   ├── pfserialize.c
│   ├── config.m4
│   └── php_functions.c
│
├── tests/
│   ├── compatibility/
│   ├── roundtrip/
│   ├── malformed/
│   └── regression/
│
├── benchmarks/
│   ├── serialize.php
│   ├── igbinary.php
│   ├── json.php
│   └── pfserialize.php
│
└── docs/
    ├── protocol.md
    ├── architecture.md
    └── benchmark.md
```

---

# 9. 数据类型

V1 必须支持：

```text
NULL
BOOL
INT
DOUBLE
STRING
ARRAY
```

例如：

```php
$data = [
    'id' => 10001,
    'name' => 'Tom',
    'price' => 99.99,
    'active' => true,
    'extra' => null
];
```

必须能够：

```text
PHP
 ↓
PFSerialize
 ↓
Binary
 ↓
PFDeserialize
 ↓
PHP
```

并保证：

```text
type(decode(encode(x))) == type(x)
```

---

# 10. Integer 编码

不能简单使用：

```text
PHP int → 8 byte
```

应该根据实际范围使用：

```text
0 ~ 127
   ↓
1 byte

128 ~ 32767
   ↓
2 byte

更大
   ↓
4 / 8 byte
```

或者使用：

```text
VarInt
```

目标：

```text
1
10
100
1000
100000
```

这些常见业务 ID 尽可能使用最少字节。

这也是 igbinary 已经采用的核心优化之一：整数使用尽可能小的 primitive 类型。([GitHub][1])

---

# 11. String Table

这是 PFSerialize 的核心功能。

例如：

```php
[
    ['id'=>1, 'status'=>'active'],
    ['id'=>2, 'status'=>'active'],
    ['id'=>3, 'status'=>'active']
]
```

不能反复写：

```text
status
active
status
active
status
active
```

而应该：

```text
String Table

0 = "status"
1 = "active"
```

实际数据：

```text
KEY_REF 0
STRING_REF 1
```

目标：

> 重复字符串只存一次。

---

# 12. PHP Array 优化

PHP Array 实际上是非常昂贵的数据结构。

因此需要区分：

### Packed Array

```php
[
    1,
    2,
    3,
    4
]
```

使用：

```text
PACKED_ARRAY
```

而不是：

```text
HASH_ARRAY
```

---

### Associative Array

```php
[
    'id' => 1,
    'name' => 'Tom'
]
```

使用：

```text
HASH_ARRAY
```

这样 Decoder 可以直接预分配 PHP HashTable。

目标：

> 尽可能减少 PHP HashTable 扩容。

---

# 13. Decoder 核心优化

这是整个项目最重要的模块。

错误方式：

```text
Binary
 ↓
Rust Value
 ↓
转换
 ↓
PHP zval
```

正确方向：

```text
Binary
 ↓
Rust Decoder
 ↓
直接创建 PHP zval
 ↓
直接填充 HashTable
```

尽可能：

```text
一次解析
一次构造
少复制
少 malloc
```

---

# 14. String Zero-Copy

对于字符串：

```text
Redis Buffer
      ↓
Decoder
      ↓
PHP String
```

需要研究：

```text
是否可以共享 buffer
```

但这里必须非常谨慎。

因为 PHP 字符串有自己的生命周期和内存管理。

所以 V1：

> **优先正确性，不强行 zero-copy。**

V2 再研究：

```text
borrowed buffer
reference counted buffer
custom allocator
```

---

# 15. Object 支持

这是 V2。

需要支持：

```text
class
public property
protected property
private property
```

同时处理：

```text
__sleep()
__wakeup()
Serializable
```

兼容性问题。

igbinary 本身支持 PHP serializer 的主要数据类型以及对象相关机制，因此如果目标是“完全替代 igbinary”，Object 兼容会成为主要工作量。([GitHub][1])

---

# 16. Reference 支持

PHP：

```php
$a = [];
$b =& $a;
```

以及：

```php
$a['self'] =& $a;
```

必须避免：

```text
无限递归
```

因此 Decoder 必须维护：

```text
Reference Table
```

例如：

```text
Reference ID
     ↓
PHP zval
```

---

# 17. PFBinary 协议

建议设计：

```text
┌──────────────┐
│ Magic        │ 4 bytes
├──────────────┤
│ Version      │ 1 byte
├──────────────┤
│ Flags        │ 1 byte
├──────────────┤
│ String Table │ variable
├──────────────┤
│ Value        │ variable
└──────────────┘
```

例如：

```text
PFBN
01
flags
...
```

---

# 18. Version

必须支持：

```text
PFBinary v1
PFBinary v2
```

Decoder：

```text
if version == 1:
    decode_v1()

if version == 2:
    decode_v2()
```

这样以后可以升级协议。

---

# 19. 数据完整性

建议支持：

```text
CRC32
```

或者：

```text
XXH3
```

用途：

```text
Redis 数据损坏
网络数据损坏
错误版本
非法数据
```

可以快速发现。

---

# 20. 安全设计

Decoder 必须防御：

```text
超大字符串
超大数组
无限递归
恶意嵌套
整数溢出
长度溢出
非法 Tag
非法 Reference
```

必须支持：

```php
pfserialize_decode(
    $data,
    $max_depth,
    $max_size
);
```

例如：

```php
$data = pfserialize_decode(
    $binary,
    100,
    10 * 1024 * 1024
);
```

---

# 21. 不允许直接执行 PHP Object

第一版必须：

```text
禁止自动执行任意对象行为
```

也就是说：

```text
Decoder
 ↓
Data
```

不能：

```text
Decoder
 ↓
执行任意 __wakeup()
```

除非明确进入兼容模式。

这是安全边界。

---

# 22. PHP API

第一版：

```php
pfserialize_encode($value);
```

```php
pfserialize_decode($binary);
```

---

增加：

```php
pfserialize_size($value);
```

返回：

```text
serialized size
```

---

Benchmark：

```php
pfserialize_stats($value);
```

返回：

```php
[
    'original_size' => 120000,
    'serialized_size' => 45000,
    'compression_ratio' => 0.375,
    'string_count' => 30
]
```

---

# 23. Redis 集成

第一阶段：

```php
$redis->set(
    'user:10001',
    pfserialize_encode($data)
);
```

第二阶段：

提供：

```php
pfredis_set(
    $redis,
    'user:10001',
    $data
);
```

读取：

```php
$data = pfredis_get(
    $redis,
    'user:10001'
);
```

这样用户甚至不需要手动 encode/decode。

---

# 24. Redis Serializer Handler

长期目标：

```text
PHP Redis
   ↓
PFSerialize
```

实现类似：

```text
Redis serializer
```

这样业务代码：

```php
$redis->set('user:10001', $user);
```

不需要：

```php
serialize()
```

而由扩展自动处理。

---

# 25. 兼容模式

必须提供：

```text
PF_MODE_NATIVE
PF_MODE_IGBINARY
PF_MODE_JSON
PF_MODE_PFBINARY
```

例如：

```php
pfserialize_set_mode(
    PF_MODE_PFBINARY
);
```

---

# 26. IgBinary 兼容

这里我建议分成两个等级。

## Level 1

能够：

```text
读取 igbinary
```

## Level 2

能够：

```text
写出 igbinary
```

## Level 3

完全兼容：

```text
igbinary ↔ PFSerialize
```

但：

> **不要在 V1 就把“完全兼容 igbinary”作为阻塞条件。**

否则项目很容易从：

```text
高性能 serializer
```

变成：

```text
igbinary 协议兼容项目
```

开发周期会大幅增加。

---

# 27. Redis 数据迁移

必须支持：

```text
旧数据
 ↓
igbinary
 ↓
PFSerialize
```

迁移工具：

```bash
pfserialize migrate \
    --redis redis://127.0.0.1:6379 \
    --pattern "user:*"
```

执行：

```text
GET
 ↓
decode old
 ↓
encode new
 ↓
SET
```

---

# 28. 灰度迁移

这是生产环境非常重要的功能。

配置：

```text
read:
    PFSerialize
    ↓
    fail
    ↓
    igbinary

write:
    PFSerialize
```

流程：

```text
                    GET
                     │
                     ▼
                PFSerialize
                     │
             ┌───────┴───────┐
             │               │
           success          fail
             │               │
             │           igbinary
             │               │
             └───────┬───────┘
                     ▼
                   PHP
```

这样可以做到：

> 不清 Redis，不停机迁移。

---

# 29. Benchmark 系统

必须自带 benchmark。

对比：

```text
serialize
unserialize

JSON
json_decode

igbinary
igbinary_unserialize

PFSerialize
pfserialize_decode
```

测试数据：

### Dataset A

```text
scalar
```

### Dataset B

```text
small associative array
```

### Dataset C

```text
1000 rows database result
```

### Dataset D

```text
deep nested array
```

### Dataset E

```text
重复字符串
```

### Dataset F

```text
packed numeric array
```

### Dataset G

```text
object collection
```

---

# 30. Benchmark 指标

必须统计：

```text
Encode latency
Decode latency

P50
P95
P99

Payload size

PHP peak memory

Rust heap allocation

CPU cycles

Redis GET

Redis SET

Network bytes
```

最终输出：

```text
                    serialize   JSON   igbinary   PFSerialize

Encode              1.00x       ...
Decode              1.00x       ...
Size                100%        ...
Memory              ...
P99                 ...
```

Redis 官方也建议通过 benchmark、CPU profiling、latency monitoring 等方式进行 Redis 性能优化，而不是单看单个操作耗时。([Redis][4])

---

# 31. 真实 Redis Benchmark

不能只测试：

```text
encode()
decode()
```

必须测试：

```text
PHP
 ↓
Redis SET
 ↓
Redis GET
 ↓
decode
```

因为真正的优化目标是：

```text
End-to-End Latency
```

---

# 32. Redis Memory Benchmark

例如：

```text
100000 keys
```

分别写入：

```text
serialize
JSON
igbinary
PFSerialize
```

然后：

```bash
redis-cli INFO memory
```

以及：

```bash
redis-cli MEMORY USAGE key
```

统计：

```text
平均
P50
P95
P99
最大
```

注意：Redis 自身还有针对小 Hash/List/Set/ZSet 的内部紧凑编码，因此不能把“序列化数据大小”直接等同于 Redis 总内存占用；最终必须测 Redis 实例真实 memory usage。([Redis][5])

---

# 33. 性能 KPI

第一版建议设成：

| 指标                  |       V1 目标 |
| ------------------- | ----------: |
| 比 serialize 小       |        ≥30% |
| Decode              | ≥ serialize |
| Decode vs igbinary  |         不低于 |
| Encode vs serialize |       ≤1.2x |
| 错误数据检测              |        100% |
| Roundtrip           |        100% |
| PHP 5.6             |          必须 |
| Linux x86_64        |          必须 |
| Redis               |          必须 |

---

# 34. V2 KPI

目标：

```text
PFSerialize
```

相比：

```text
igbinary
```

达到：

```text
Payload
-10% ~ -20%

Decode
+20% ~ +30%

P99 latency
-15%+

CPU
-15%+
```

但这些必须通过实际数据验证。

---

# 35. 不应该优化的东西

第一版明确禁止：

```text
SIMD
AVX
GPU
复杂压缩
对象魔术方法
PHP 8 特性
跨语言协议
```

原因很简单：

> **先证明 serializer 本身有优势。**

---

# 36. V1 MVP

我建议 V1 只做：

```text
PHP 5.6
+
Rust
+
Linux
+
NULL
+
BOOL
+
INT
+
DOUBLE
+
STRING
+
ARRAY
+
String Table
+
Packed Array
+
VarInt
+
Fast Decoder
+
Benchmark
```

不做：

```text
Object
Reference
Compression
Redis migration
igbinary compatibility
```

---

# 37. V1 开发顺序

### Phase 1

```text
Rust Core
```

完成：

```text
Encoder
Decoder
Binary Format
```

---

### Phase 2

```text
PHP Extension
```

实现：

```php
pfserialize_encode()
pfserialize_decode()
```

---

### Phase 3

```text
Benchmark
```

和：

```text
serialize
JSON
igbinary
```

比较。

---

### Phase 4

```text
真实 Redis
```

跑：

```text
10万
100万
1000万
```

key。

---

### Phase 5

根据 benchmark 决定：

```text
继续
```

还是：

```text
停止
```

这一点非常重要。

---

# 38. V2

加入：

```text
Object
Reference
IgBinary compatibility
Redis migration
Compression
```

---

# 39. V3

真正做成基础设施：

```text
PHP Redis serializer handler
```

实现：

```php
$redis->set('foo', $array);
$array = $redis->get('foo');
```

业务代码不需要变化。

---

# 40. V4

加入：

```text
Redis Analyzer
```

例如：

```bash
pfredis analyze
```

输出：

```text
Redis Memory Analysis

Total memory:
128 GB

PHP serialized:
83 GB

JSON:
12 GB

igbinary:
27 GB

PFSerialize:
6 GB

Top 100 largest keys:
...

Top repeated strings:
...

Potential saving:
52 GB
```

这时候项目就不再只是 serializer 了。

---

# 41. 最终产品形态

我认为最终应该形成：

```text
PFSerialize
│
├── PHP Extension
│
├── Rust Serialization Core
│
├── Redis Integration
│
├── Migration Tool
│
├── Benchmark Tool
│
└── Redis Analyzer
```

---

# 42. 最终商业价值

最终真正可以卖的不是：

> “一个比 igbinary 快的 serializer。”

而是：

> **“帮助 PHP 老系统降低 Redis 内存和缓存延迟的基础设施。”**

比如一个客户：

```text
Redis
300 GB
```

经过分析：

```text
PHP serialize
 ↓
PFSerialize
```

变成：

```text
300 GB
 ↓
180 GB
```

那么客户真正关心的是：

```text
Redis 实例从 384GB
 ↓
256GB
```

节省的是：

```text
服务器成本
+
网络成本
+
Redis 成本
+
CPU
```

这才是产品价值。

---

# 43. 一个我认为非常重要的战略调整

**不要把“兼容 igbinary”作为项目核心。**

项目应该形成三层：

```text
                 PFSerialize
                     │
       ┌─────────────┼──────────────┐
       │             │              │
   Compatibility   Performance    Analytics
       │             │              │
   igbinary       Rust Core       Redis
   serialize      Decoder         Analyzer
```

其中：

**Compatibility 是迁移工具。**

**Performance 是核心技术。**

**Analytics 是商业价值。**

---

# 44. 最终一句话定义

如果让我给这个项目写 GitHub README 第一行，我会写：

> **PFSerialize is a Rust-powered binary serialization engine for legacy PHP applications, optimized for Redis-heavy workloads with a focus on memory efficiency, network reduction, and read-path decoding performance.**

中文：

> **PFSerialize 是一个由 Rust 驱动、专门针对 PHP 老系统 Redis 缓存场景优化的高性能二进制序列化引擎，重点降低缓存内存、网络传输和反序列化延迟。**

---

## 我对这个项目的建议

你这个项目**值得做，但第一阶段千万别直接写 Rust Encoder**。

正确的第一步应该是：

```text
① 收集真实 PHP Redis 数据
        ↓
② 建立 serialize / JSON / igbinary baseline
        ↓
③ 分析 10~20 种真实数据结构
        ↓
④ 找出 igbinary 的性能瓶颈
        ↓
⑤ 再设计 PFBinary
        ↓
⑥ Rust 实现
```

因为现在已经有其他项目开始尝试针对 PHP cache workload 做 decoder-oriented serializer，这说明“**把优化重点放在 decode、packed numeric arrays、nested structures、重复字符串以及 PHP 内存布局**”确实是一个有竞争空间的方向，而不是凭空假设。([GitHub][6])

**下一步最有价值的工作不是写代码，而是做一份《PFSerialize 技术架构设计 v1.0》**：把 **PFBinary 二进制协议、Tag 定义、VarInt、String Table、Array Layout、Reference Table、PHP 5.6 zval 映射、Rust/C ABI、Redis 集成方式**全部定下来。这个确定以后，基本就可以直接进入 Rust 编码阶段。

[1]: https://github.com/igbinary/igbinary?utm_source=chatgpt.com "GitHub - igbinary/igbinary: Igbinary is a drop in replacement for the standard php serializer. · GitHub"
[2]: https://github.com/igbinary/igbinary/blob/master/benchmark/comparisons.php?utm_source=chatgpt.com "igbinary/benchmark/comparisons.php at master · igbinary/igbinary · GitHub"
[3]: https://www.zend.com/resources/php-extensions?utm_source=chatgpt.com "Writing PHP Extensions | Zend"
[4]: https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/?utm_source=chatgpt.com "Optimizing Redis | Docs"
[5]: https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/memory-optimization/?utm_source=chatgpt.com "Memory optimization | Docs"
[6]: https://github.com/iliaal/phpser?utm_source=chatgpt.com "GitHub - iliaal/phpser: Fast binary serializer for PHP cache workloads. Decoder-optimized, beats igbinary on packed numerics, deep-nested structures, and same-class DTO batches. · GitHub"
