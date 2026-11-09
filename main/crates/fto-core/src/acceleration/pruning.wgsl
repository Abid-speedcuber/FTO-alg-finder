@group(0) @binding(0) var<storage, read> transitions: array<u32>;
@group(0) @binding(1) var<storage, read> p: array<u32>;
@group(0) @binding(2) var<storage, read_write> visited: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read> frontier: array<u32>;
@group(0) @binding(4) var<storage, read_write> next_frontier: array<atomic<u32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p[4] { return; }
    let index = frontier[id.x];
    if index >= p[0] { return; }
        var coordinates: array<u32, 13>;
        var remaining = index;
        for (var c = p[2]; c > 0u; c--) {
            let size = p[8u + (c - 1u) * 2u];
            coordinates[c - 1u] = remaining % size;
            remaining /= size;
        }
        for (var m = 0u; m < p[1]; m++) {
            var child = 0u;
            for (var c = 0u; c < p[2]; c++) {
                let size = p[8u + c * 2u];
                let offset = p[9u + c * 2u];
                child = child * size + transitions[offset + coordinates[c] * p[1] + m];
            }
            let flag = 1u << (child & 31u);
            if (atomicOr(&visited[child >> 5u], flag) & flag) == 0u {
                atomicOr(&next_frontier[child >> 5u], flag);
            }
        }

}
