pub struct Util {}

impl Util {
    pub fn pack_floats(a: f32, b: f32) -> u32 {
        let ha = half::f16::from_f32(a);
        let hb = half::f16::from_f32(b);
        ((hb.to_bits() as u32) << 16) | (ha.to_bits() as u32)
    }
}
