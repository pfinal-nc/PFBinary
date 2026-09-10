<?php
/**
 * Hardened regression: protocol goldens, malformed, boundaries.
 *
 *   php -d extension=php/modules/pfserialize.so php/tests/regression.php
 */
declare(strict_types=1);

if (!function_exists('pfserialize_encode')) {
    fwrite(STDERR, "pfserialize not loaded\n");
    exit(1);
}

$fail = 0;
function expect_false(string $name, mixed $got): void
{
    global $fail;
    if ($got !== false) {
        fwrite(STDERR, "FAIL $name: expected false\n");
        $fail++;
    } else {
        echo "OK $name\n";
    }
}

function expect_true(string $name, bool $cond, string $detail = ''): void
{
    global $fail;
    if (!$cond) {
        fwrite(STDERR, "FAIL $name" . ($detail !== '' ? ": $detail" : '') . "\n");
        $fail++;
    } else {
        echo "OK $name\n";
    }
}

echo "pfserialize regression (PHP " . PHP_VERSION . ")\n";

/* --- Protocol goldens (docs/protocol-v1 + Rust pf-core) --- */
$sampleA = [
    'id' => 10001,
    'name' => 'Tom',
    'price' => 99.99,
    'active' => true,
    'extra' => null,
];
$hexA = bin2hex(pfserialize_encode($sampleA));
$wantA = '5046424e010209050602696403a29c0106046e616d650603546f6d06057072696365048fc2f5285cff58400606616374697665020605657874726100';
expect_true('golden sample A', $hexA === $wantA, "got $hexA");

$sampleB = [
    ['id' => 1, 'status' => 'active'],
    ['id' => 2, 'status' => 'active'],
    ['id' => 3, 'status' => 'active'],
];
$binB = pfserialize_encode($sampleB);
expect_true('sample B tag ARRAY_ROWS', ord($binB[6]) === 0x0B, 'tag=' . bin2hex($binB[6]));
expect_true('sample B active once', substr_count($binB, 'active') === 1);
expect_true('sample B roundtrip', pfserialize_decode($binB) === $sampleB);

$sampleC = [1, 2, 3, 4];
$hexC = bin2hex(pfserialize_encode($sampleC));
expect_true('golden sample C PACKED_LONGS', $hexC === '5046424e01020a0402040608', "got $hexC");

$sampleD = array_fill(0, 50, ['id' => 1, 'status' => 'active', 'role' => 'member']);
$binD = pfserialize_encode($sampleD);
expect_true('sample D tag ARRAY_REPEAT', ord($binD[6]) === 0x0C);
expect_true('sample D size < 80', strlen($binD) < 80, 'len=' . strlen($binD));
expect_true('sample D roundtrip', pfserialize_decode($binD) === $sampleD);

/* --- Rejects --- */
expect_false('reject object', @pfserialize_encode(new stdClass()));
$res = fopen('php://memory', 'r');
expect_false('reject resource', @pfserialize_encode($res));
if (is_resource($res)) {
    fclose($res);
}
expect_false('bad magic', @pfserialize_decode("XXXX\x01\x02\x00"));
expect_false('truncated header', @pfserialize_decode('PFB'));
expect_false('unsupported version', @pfserialize_decode("PFBN\x02\x02\x00"));
expect_false('unknown flags', @pfserialize_decode("PFBN\x01\x80\x00"));

$ok = pfserialize_encode(['x' => 1]);
$chk = $ok;
$chk[5] = chr(ord($chk[5]) | 0x01);
expect_false('checksum flag rejected', @pfserialize_decode($chk));

$trail = pfserialize_encode(1) . "\x00";
expect_false('trailing bytes', @pfserialize_decode($trail));

$unknownTag = "PFBN\x01\x02\x0D";
expect_false('unknown tag 0x0D', @pfserialize_decode($unknownTag));

$deep = [[[[[1]]]]];
$deepBin = pfserialize_encode($deep);
expect_false('max_depth', @pfserialize_decode($deepBin, 2));
expect_true('max_depth ok higher', pfserialize_decode($deepBin, 10) === $deep);

$big = range(1, 5000);
$bigBin = pfserialize_encode($big);
expect_false('max_size too small', @pfserialize_decode($bigBin, 100, 64));
expect_true('max_size ok', is_array(pfserialize_decode($bigBin, 100, strlen($bigBin))));

/* --- Malformed / truncated fuzz (should never crash or leak) --- */
$corpus = [
    '',
    'P',
    'PFBN',
    "PFBN\x01",
    "PFBN\x01\x02",
    "PFBN\x01\x02\x0A",
    "PFBN\x01\x02\x0A\xFF",
    "PFBN\x01\x02\x09\x05",
    "PFBN\x01\x02\x0B\x01\x01",
    "PFBN\x01\x02\x0C\x01",
    "PFBN\x01\x02\x06\xFF",
    "PFBN\x01\x02\x07\x00",
    "PFBN\x01\x02\x03" . str_repeat("\xFF", 16),
    $ok . "\xFF",
    substr($ok, 0, -1),
];
foreach ($corpus as $i => $blob) {
    expect_false("fuzz#$i", @pfserialize_decode($blob));
}

/* --- ZigZag / int edges --- */
foreach ([0, -1, 1, 63, -64, 64, 10001, PHP_INT_MAX, PHP_INT_MIN] as $n) {
    $b = pfserialize_encode($n);
    expect_true("int edge $n", pfserialize_decode($b) === $n);
}

/* --- Binary string / empty --- */
foreach (['', "a\0b\xff", str_repeat('中', 100)] as $i => $s) {
    expect_true("string#$i", pfserialize_decode(pfserialize_encode($s)) === $s);
}

if ($fail > 0) {
    fwrite(STDERR, "FAILED: $fail\n");
    exit(1);
}
echo "ALL PASS\n";
