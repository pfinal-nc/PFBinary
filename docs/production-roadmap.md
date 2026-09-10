# PFSerialize 生产路线图（Decode 优先）

**状态：** 执行中  
**依据：** [README.md](../README.md) §3–5、§13、§33、§37 Phase 5  
**门槛原则：** 达不到门禁 = 不上生产、不宣称可替换 igbinary。

---

## 1. 生产定义（README）

上生产不是“能 encode/decode”，而是：

> Redis 读多写少场景下：**更小体积 + decode 不低于 igbinary**，并最终挑战更快。

| 门禁 | V1 生产最低线 | 优秀线 |
|------|---------------|--------|
| G1 体积 vs serialize | 典型 workload 平均 ≤ 70%（降 ≥30%） | ≤ 60% |
| G2 Decode vs serialize | ≥ serialize | — |
| G3 Decode vs igbinary | **≥ igbinary（不低于）** | +20% |
| G4 Encode vs serialize | ≤ 1.2× | — |
| G5 Roundtrip / malformed | 100% | — |
| G6 Redis e2e | SET/GET + decode 延迟可测 | — |

**当前（纯 C 扩展，无 Rust 链接）：** G1/G3/G4/G5 达标；扩展仅编 `encode_c.c`/`decode_c.c`，**不链接** `libpfserialize`。Checksum 载荷显式拒绝。G6 Redis e2e / Linux CI 仍待做。

---

## 2. 根因（已验证）

```text
现状：Binary → Rust → 每节点 FFI 回调 → emalloc(zval) → HashTable
目标：Binary → 紧循环 → 栈上 zval / 直写 arPacked → HashTable
```

igbinary 赢在：**零 per-scalar 堆分配 + 纯 C 紧循环**。  
不消灭这条路径上的 `emalloc`/FFI，不可能上生产。

---

## 3. 改造阶段

### P0 — Decode 直写（本阶段，阻塞）

1. Host 增加数组内原始类型写入：`push_i64` / `push_bool` / `push_null` / `push_f64` / `push_string` / `insert_*_key_*`
2. Decoder 在 `ARRAY_*` 内对标量走直写，**不**再 `make_*` + 堆 `Value`
3. PHP Host：栈上 `zval` + `zend_hash_*`（`ZVAL_COPY_VALUE` 语义）
4. 仅嵌套 array 作为完整 `Value` 传递（次数远少于标量）
5. 用 `php/examples/compare.php` 回归；**G3 未过则继续抠 P0，不上新功能**

### P1 — 协议热路径

1. ~~实现 `PACKED_LONGS`（tag `0x0A`）~~ **已落地**（encode 全 int list + decode `push_i64_run`）
2. ~~重复行 / 同质结构~~ → **`ARRAY_ROWS`（`0x0B`）+ `ARRAY_REPEAT`（`0x0C`）已落地**
3. ~~体积门禁 G1 在 `repeated_keys` 上追平或接近 igbinary~~ → **已优于 ig**

### P2 — 生产硬化

1. Linux CI + PHP 8.x（本仓库当前目标；5.6 另轨）
2. Redis e2e bench（README §31）
3. ~~符号可见性 / 静态链接或 rpath 安装，去掉手动 `DYLD_LIBRARY_PATH`~~ → **已落地：PHP 扩展纯 C，不链接 Rust**
4. 灰度读：Magic `PFBN` 失败回退 igbinary（应用层或扩展层）

### P1′ — 交付与回归（本机已补）

1. ~~安装交付~~ → `php/install.sh` + `php/pfserialize.ini` + `php/README.md`
2. ~~硬化回归~~ → `php/tests/regression.php`（协议 goldens / 畸形 / 边界）+ `php/tests/run_all.sh`
3. ~~业务向样例~~ → `php/examples/workload_bench.php`

### 明确不做（未过 G3 前）

- Redis serializer handler、迁移工具、Analyzer、Object/Reference、SIMD

---

## 4. 架构决策（生产）

| 决策 | 选择 | 理由 |
|------|------|------|
| 协议权威 | 仍在 Rust `pf-core` / `pf-format`（单测 / CLI） | 不依赖 PHP；与扩展热路径解耦 |
| Decode 热路径 | **PHP 扩展纯 C**；checksum 载荷拒绝 | 无 FFI；V1 不在 PHP 侧实现 XXH3 |
| Encode | **PHP 扩展纯 C**；checksum flag 拒绝 | 无 FFI；G4 ≤ serialize×1.2 |
| 若 C 后 G3 仍差 | 协议层或微优化 | 当前三数据集已过 |

---

## 5. 验收命令

```bash
bash php/build.sh
php -d extension=php/modules/pfserialize.so php/examples/compare.php
# igbinary 对比时：
php -d extension=igbinary -d extension=php/modules/pfserialize.so php/examples/compare.php
```

（纯 C 扩展，**不需要** `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH`。）

**P0 通过标准（人工判读 compare 输出）：**

- `packed_1k` / `rows_100` / `repeated_keys`：pf decode **≤ igbinary decode × 1.0**（不低于）
- 体积：不显著差于改造前；packed 保持优于 serialize≥30%

---

## 6. 变更记录

| 日期 | 说明 |
|------|------|
| 2026-09-10 | 确认生产路线；P0 Decode 直写启动 |
| 2026-09-10 | P1：`PACKED_LONGS` 落地；`packed_1k` decode/体积过 igbinary；G3 整体仍未过 |
| 2026-09-10 | Host `intern_string` + `StringId`：rows/repeated decode 约减半，G3 仍未过；下一步评估 C 内联解码 |
| 2026-09-10 | **C 内联解码器**落地（`php/decode_c.c`）；`packed_1k` 稳胜；`rows_100` 近持平；`repeated_keys` 仍受体积拖累 |
| 2026-09-10 | `ARRAY_ROWS` + `ARRAY_REPEAT`：compare 三数据集 G3 过线；`repeated_keys` 体积 46≪ig 242 |
| 2026-09-10 | C 编码器 + 静态链接：G4 过线；运行不再依赖 `DYLD_LIBRARY_PATH` |
| 2026-09-10 | **纯 C 收尾**：去掉 PHP→Rust FFI/`libpfserialize`；checksum 显式拒绝；`host_php8.c` 薄封装 |
| 2026-09-10 | **P1′ 交付**：install/ini、regression、workload_bench；杀手能力见业务向样例 |
