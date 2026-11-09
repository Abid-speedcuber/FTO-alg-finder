@group(0) @binding(0) var<storage, read> transitions: array<u32>;
@group(0) @binding(1) var<storage, read> p: array<u32>;
@group(0) @binding(2) var<storage, read> jobs: array<u32>;
@group(0) @binding(3) var<storage, read_write> output_hashes: array<u32>;
@group(0) @binding(4) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> bloom_bits: array<atomic<u32>>;

// WGSL's portable path has u32, so implement the cache's stable u64 hash exactly.
fn high_mul(a: u32, b: u32) -> u32 {
    let a0 = a & 65535u; let a1 = a >> 16u;
    let b0 = b & 65535u; let b1 = b >> 16u;
    let t0 = a0 * b0;
    let t1 = a1 * b0 + (t0 >> 16u);
    let t2 = a0 * b1 + (t1 & 65535u);
    return a1 * b1 + (t1 >> 16u) + (t2 >> 16u);
}
fn mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return vec2<u32>(a.x * b.x, high_mul(a.x, b.x) + a.x * b.y + a.y * b.x);
}
fn shr64(a: vec2<u32>, n: u32) -> vec2<u32> {
    return vec2<u32>((a.x >> n) | (a.y << (32u - n)), a.y >> n);
}
fn mix64(value: vec2<u32>) -> vec2<u32> {
    var x = mul64(value ^ shr64(value, 30u), vec2<u32>(0x1ce4e5b9u, 0xbf58476du));
    x = mul64(x ^ shr64(x, 27u), vec2<u32>(0x133111ebu, 0x94d049bbu));
    return x ^ shr64(x, 31u);
}
fn add64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let low = a.x + b.x;
    return vec2<u32>(low, a.y + b.y + select(0u, 1u, low < a.x));
}
fn modulo64(value: vec2<u32>, divisor: vec2<u32>) -> vec2<u32> {
    // The common power-of-two case needs no long division.
    if p[19] == 1u { return value & vec2<u32>(p[20], p[21]); }
    var remainder = vec2<u32>(0u);
    for (var n = 64u; n > 0u; n--) {
        let index = n - 1u;
        var bit: u32;
        if index >= 32u { bit = (value.y >> (index - 32u)) & 1u; }
        else { bit = (value.x >> index) & 1u; }
        remainder = vec2<u32>((remainder.x << 1u) | bit, (remainder.y << 1u) | (remainder.x >> 31u));
        if remainder.y > divisor.y || (remainder.y == divisor.y && remainder.x >= divisor.x) {
            let borrow = select(0u, 1u, remainder.x < divisor.x);
            remainder = vec2<u32>(remainder.x - divisor.x, remainder.y - divisor.y - borrow);
        }
    }
    return remainder;
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p[6] { return; }
    let ordinal = p[5] + id.x;
    let job = ordinal / p[3];
    if job >= p[4] { return; }
    var sequence = ordinal % p[3];
    var length = 0u; var layer_size = 1u;
    while sequence >= layer_size {
        sequence -= layer_size; length++; layer_size *= p[1];
    }
    var state: array<u32, 10>;
    for (var c = 0u; c < 10u; c++) { state[c] = jobs[job * 11u + c]; }
    var last = jobs[job * 11u + 10u];
    for (var step = 0u; step < length; step++) {
        let mv = sequence % p[1]; sequence /= p[1];
        let allowed = p[32u + last * 2u + (mv >> 5u)];
        if (allowed & (1u << (mv & 31u))) == 0u { return; }
        var child: array<u32, 10>;
        for (var c = 0u; c < 7u; c++) { child[c] = transitions[p[8u + c] + state[c] * p[1] + mv]; }
        if p[0] == 1u {
            for (var c = 7u; c < 10u; c++) { child[c] = transitions[p[15] + mv * 12u + state[c]]; }
        }
        state = child; last = mv;
    }
    let a = vec2<u32>(state[0] | (state[1] << 16u), state[2] | (state[3] << 16u));
    var h: vec2<u32>;
    if p[0] == 0u {
        let b = vec2<u32>(state[4] | (state[6] << 20u), (state[6] >> 12u) | (state[5] << 8u));
        h = mix64(a ^ mix64(b ^ vec2<u32>(0x7f4a7c15u, 0x9e3779b9u)));
    } else {
        let b = vec2<u32>(state[4] | (state[5] << 16u), state[6]);
        let c = vec2<u32>(state[7] | (state[8] << 4u) | (state[9] << 8u), 0u);
        h = mix64(a ^ mix64(b ^ mix64(c ^ vec2<u32>(0x4c4f4f4du, 0x4c4c5f42u))));
    }
    let delta = mix64(h ^ vec2<u32>(0xd192ed03u, 0xd1b54a32u)) | vec2<u32>(1u, 0u);
    let out = atomicAdd(&counters[0], 1u);
    if p[16] != 0u {
        var probe = h;
        let size = vec2<u32>(p[17], p[18]);
        for (var k = 0u; k < 7u; k++) {
            let bit = modulo64(probe, size);
            let index = (bit.x >> 5u) | (bit.y << 27u);
            let flag = 1u << (bit.x & 31u);
            if (atomicOr(&bloom_bits[index], flag) & flag) == 0u {
                let old = atomicAdd(&counters[2], 1u);
                if old == 0xffffffffu { atomicAdd(&counters[3], 1u); }
            }
            probe = add64(probe, delta);
        }
    }
    if p[16] != 1u {
        if out >= p[7] { atomicStore(&counters[1], 1u); return; }
        output_hashes[out * 4u] = h.x; output_hashes[out * 4u + 1u] = h.y;
        output_hashes[out * 4u + 2u] = delta.x; output_hashes[out * 4u + 3u] = delta.y;
    }
}
