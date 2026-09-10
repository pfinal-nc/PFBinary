<?php
/**
 * 业务向 workload：pfserialize vs serialize（读多写少缓存形态）
 *
 *   php -d extension=php/modules/pfserialize.so php/examples/workload_bench.php
 *   php -d extension=php/modules/pfserialize.so php/examples/workload_bench.php 2000
 */
declare(strict_types=1);

if (!function_exists('pfserialize_encode')) {
    fwrite(STDERR, "pfserialize not loaded\n");
    exit(1);
}

$rounds = isset($argv[1]) ? max(100, (int)$argv[1]) : 2000;

function mk_user(int $i): array
{
    return [
        'uid' => 100000 + $i,
        'name' => 'user_' . $i,
        'email' => "u{$i}@example.com",
        'roles' => ['member', 'reader'],
        'vip' => ($i % 7) === 0,
        'created_at' => 1700000000 + $i,
    ];
}

function mk_sku(int $i): array
{
    return [
        'sku_id' => 900000 + $i,
        'title' => 'SKU-' . $i,
        'price' => 9.9 + ($i % 50),
        'stock' => 100 + ($i % 20),
        'cats' => ['home', 'sale'],
        'attrs' => ['color' => 'red', 'size' => 'M'],
    ];
}

$workloads = [
    // Session / auth cache
    'session_user' => [
        'sid' => 'sess_' . str_repeat('a', 26),
        'user' => mk_user(42),
        'cart_count' => 3,
        'flash' => null,
    ],
    // Product list page (homogeneous rows — ARRAY_ROWS / REPEAT friendly)
    'plp_50' => array_map('mk_sku', range(1, 50)),
    'plp_200' => array_map('mk_sku', range(1, 200)),
    // Feed / timeline items with repeated keys
    'feed_100' => array_map(
        static fn(int $i): array => [
            'id' => $i,
            'type' => 'post',
            'author' => 'u' . ($i % 10),
            'text' => 'hello #' . $i,
            'ts' => 1700000000 + $i,
        ],
        range(1, 100)
    ),
    // Config / feature flags (small, hot decode)
    'feature_flags' => [
        'new_checkout' => true,
        'search_v2' => false,
        'ab' => ['exp_a' => 0.5, 'exp_b' => 0.5],
        'limits' => ['qps' => 1000, 'batch' => 50],
    ],
    // ID list cache (PACKED_LONGS)
    'id_list_2k' => range(10001, 12000),
    // Nested permission tree-ish
    'acl_tree' => [
        'roles' => [
            'admin' => ['read' => true, 'write' => true, 'delete' => true],
            'member' => ['read' => true, 'write' => false, 'delete' => false],
            'guest' => ['read' => true, 'write' => false, 'delete' => false],
        ],
        'overrides' => array_map(
            static fn(int $i): array => ['uid' => $i, 'perm' => 'read'],
            range(1, 30)
        ),
    ],
];

function bench_ms(callable $fn, int $rounds): float
{
    $fn();
    $fn();
    $t0 = hrtime(true);
    for ($i = 0; $i < $rounds; $i++) {
        $fn();
    }
    return (hrtime(true) - $t0) / 1e6 / $rounds;
}

echo "=== workload bench pf vs serialize (PHP " . PHP_VERSION . ") ===\n";
echo "rounds={$rounds}\n\n";
printf(
    "%-14s %8s %8s %7s  %9s %9s %7s  %9s %9s %7s\n",
    'workload',
    'ser_B',
    'pf_B',
    'size%',
    'ser_enc',
    'pf_enc',
    'enc↑',
    'ser_dec',
    'pf_dec',
    'dec↑'
);
echo str_repeat('-', 108) . "\n";

foreach ($workloads as $name => $data) {
    $ser = serialize($data);
    $pf = pfserialize_encode($data);
    if ($pf === false) {
        fwrite(STDERR, "encode fail: $name\n");
        exit(1);
    }
    if (serialize(pfserialize_decode($pf)) !== $ser) {
        fwrite(STDERR, "roundtrip fail: $name\n");
        exit(1);
    }

    $serEnc = bench_ms(static fn() => serialize($data), $rounds);
    $pfEnc = bench_ms(static fn() => pfserialize_encode($data), $rounds);
    $serDec = bench_ms(static fn() => unserialize($ser), $rounds);
    $pfDec = bench_ms(static fn() => pfserialize_decode($pf), $rounds);

    $serB = strlen($ser);
    $pfB = strlen($pf);
    printf(
        "%-14s %8d %8d %6.1f%%  %7.3fms %7.3fms %6.2fx  %7.3fms %7.3fms %6.2fx\n",
        $name,
        $serB,
        $pfB,
        $serB > 0 ? $pfB / $serB * 100 : 0,
        $serEnc,
        $pfEnc,
        $serEnc / max($pfEnc, 1e-12),
        $serDec,
        $pfDec,
        $serDec / max($pfDec, 1e-12)
    );
}

echo "\n↑ = serialize_ms / pf_ms（>1 表示 pf 更快）\n";
