pub struct Util {}

// https://users.rust-lang.org/t/help-with-macro-for-opengl-error-checking/20840/5
#[macro_export]
macro_rules! glchk {
    ($($s:stmt;)*) => {
        $(
            $s;
            { //if cfg!(debug_assertions) {
                let err = gl::GetError();
                if err != gl::NO_ERROR {
                    let err_str = match err {
                        gl::INVALID_ENUM => "GL_INVALID_ENUM",
                        gl::INVALID_VALUE => "GL_INVALID_VALUE",
                        gl::INVALID_OPERATION => "GL_INVALID_OPERATION",
                        gl::INVALID_FRAMEBUFFER_OPERATION => "GL_INVALID_FRAMEBUFFER_OPERATION",
                        gl::OUT_OF_MEMORY => "GL_OUT_OF_MEMORY",
                        gl::STACK_UNDERFLOW => "GL_STACK_UNDERFLOW",
                        gl::STACK_OVERFLOW => "GL_STACK_OVERFLOW",
                        _ => "unknown error"
                    };
                    println!("{}:{} - {} caused {}",
                             file!(),
                             line!(),
                             stringify!($s),
                             err_str);
                }
            }
        )*
    }
}

impl Util {
    pub fn pack_floats(a: f32, b: f32) -> u32 {
        let ha = half::f16::from_f32(a);
        let hb = half::f16::from_f32(b);
        ((hb.to_bits() as u32) << 16) | (ha.to_bits() as u32)
    }

    pub fn unpack_floats(packed: u32) -> (f32, f32) {
        let mask: u32 = 0xFFFF;
        let ha_bits = (packed & mask) as u16;
        let hb_bits = ((packed >> 16) & mask) as u16;
        let a = half::f16::from_bits(ha_bits).to_f32();
        let b = half::f16::from_bits(hb_bits).to_f32();
        (a, b)
    }
}
