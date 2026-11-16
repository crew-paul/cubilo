struct Object { matrix: mat4x4<f32>, world: mat4x4<f32>, color: vec4<f32> };
@group(0) @binding(0) var<uniform> object: Object;
struct Output { @builtin(position) position: vec4<f32>, @location(0) normal: vec3<f32> };
@vertex fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>) -> Output {
    var out: Output;
    out.position = object.matrix * vec4<f32>(position, 1.0);
    out.normal = (object.world * vec4<f32>(normal, 0.0)).xyz;
    return out;
}
@fragment fn fragment(in: Output) -> @location(0) vec4<f32> {
    let shade = 0.35 + 0.65 * max(dot(normalize(in.normal), normalize(vec3<f32>(0.4, -0.5, 1.0))), 0.0);
    let light = select(shade, 1.0, object.color.a > 1.5);
    return vec4<f32>(object.color.rgb * light, 1.0);
}
