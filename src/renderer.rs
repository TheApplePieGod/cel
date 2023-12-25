use std::{ffi::c_void, mem::size_of, ptr::{null, null_mut}};
use crate::font::Font;

const MAX_CHARACTERS: u32 = 20000;

type Vec2 = [f32; 2];
type Vec3 = [f32; 3];

#[repr(C)]
pub struct Vertex {
    position: Vec3,
    tex_coord: Vec2
}

#[repr(C)]
pub struct Offset {
    position: u32,
    scale: u32,
    tex_coord: u32
}

pub struct Renderer {
    shader_program: u32,
    quad_vao: u32,
    quad_vbo: u32,
    quad_ibo: u32,
    pos_buf: u32,
    pos_tex: u32,
    width: u32,
    height: u32
}

impl Renderer {
    pub fn new() -> Self {
        // Static quad data
        let uv_max = 1.0 - 0.05; // Account for atlas oversample
        let quad_vertices: [Vertex; 4] = [
            Vertex { position: [1.0, 1.0, 0.0], tex_coord: [uv_max, 0.0] }, // TR
            Vertex { position: [1.0, 0.0, 0.0], tex_coord: [uv_max, uv_max] }, // BR
            Vertex { position: [0.0, 0.0, 0.0], tex_coord: [0.0, uv_max] }, // BL
            Vertex { position: [0.0, 1.0, 0.0], tex_coord: [0.0, 0.0] } // TL
        ];
        let quad_indices: [u32; 6] = [
            3, 1, 0, 3, 2, 1
        ];

        // Generate buffers
        let mut quad_vao: u32 = 0;
        let mut quad_vbo: u32 = 0;
        let mut quad_ibo: u32 = 0;
        let mut pos_buf: u32 = 0;
        let mut pos_tex: u32 = 0;
        unsafe {
            // VAO
            gl::GenVertexArrays(1, &mut quad_vao);
            gl::BindVertexArray(quad_vao);

            // VBO
            gl::GenBuffers(1, &mut quad_vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, quad_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (size_of::<Vertex>() * quad_vertices.len()) as isize,
                quad_vertices.as_ptr() as *const c_void,
                gl::STATIC_DRAW
            );

            // IBO
            gl::GenBuffers(1, &mut quad_ibo);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, quad_ibo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (size_of::<u32>() * quad_indices.len()) as isize,
                quad_indices.as_ptr() as *const c_void,
                gl::STATIC_DRAW
            );

            // VAO attributes
            let vert_stride = size_of::<Vertex>() as i32;
            let pos_stride = (size_of::<f32>() * 3) as i32;
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, vert_stride, null());
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, vert_stride, pos_stride as *const c_void);
            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);

            // Position buffer
            gl::GenBuffers(1, &mut pos_buf);
            gl::BindBuffer(gl::TEXTURE_BUFFER, pos_buf);
            gl::BufferData(
                gl::TEXTURE_BUFFER,
                (size_of::<Offset>() * (MAX_CHARACTERS as usize)) as isize,
                null(),
                gl::DYNAMIC_DRAW
            );
            gl::GenTextures(1, &mut pos_tex);
        }

        let vert_shader_source = b"
            #version 400 core

            layout (location = 0) in vec3 inPos;
            layout (location = 1) in vec2 inTexCoord;

            out vec2 texCoord;

            uniform usamplerBuffer offsetTex;
            uniform mat4 model;
            uniform vec2 texCoordScale;

            uint half2float(uint h) {
                return ((h & uint(0x8000)) << uint(16)) | ((( h & uint(0x7c00)) + uint(0x1c000)) << uint(13)) | ((h & uint(0x03ff)) << uint(13));
            }

            vec2 unpackHalf2x16(uint v) {	
                return vec2(uintBitsToFloat(half2float(v & uint(0xffff))),
                        uintBitsToFloat(half2float(v >> uint(16))));
            }

            void main()
            {
                uvec4 offset = texelFetch(offsetTex, gl_InstanceID);
                vec2 pos = unpackHalf2x16(offset.x);
                vec2 scale = unpackHalf2x16(offset.y);
                vec2 coord = unpackHalf2x16(offset.z);

                gl_Position = model * (vec4(inPos * vec3(scale, 1.0), 1.0)
                    + vec4(pos, 0.f, 0.f)) // Add character-space offset
                    + vec4(-1.f, 0.f, 0.f, 0.f); // Move origin to top left 
                texCoord = inTexCoord * texCoordScale + coord;
            }
        \0";

        let frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;

            out vec4 fragColor;

            uniform sampler2D atlasTex;
            uniform float pixelRange;

            float Median(float r, float g, float b, float a) {
                return max(min(r, g), min(max(r, g), b));
            }

            void main()
            {
                vec4 msd = texture(atlasTex, texCoord);
                float sd = Median(msd.r, msd.g, msd.b, msd.a);
                float screenPxDistance = pixelRange * (sd - 0.5);
                float opacity = clamp(screenPxDistance + 0.5, 0.0, 1.0);
                
                //fragColor = mix(vec4(0.f), vec4(1.f), opacity);

                if (opacity < 0.05)
                    discard;
                fragColor = vec4(1.f, 1.f, 1.f, opacity);

                //fragColor = msd;
                //fragColor = vec4(opacity, opacity, opacity, 1.f);
                //fragColor = vec4(texCoord.r, texCoord.g, 0.f, 1.f);
            }
        \0";

        // Compile shaders & generate program
        let vert_shader = match Self::compile_shader(gl::VERTEX_SHADER, vert_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile vertex shader: {}", msg)
        };
        let frag_shader = match Self::compile_shader(gl::FRAGMENT_SHADER, frag_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile frag shader: {}", msg)
        };
        let program = match Self::link_program(vert_shader, frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link program: {}", msg)
        };

        // Free shaders
        unsafe {
            gl::DeleteShader(vert_shader);
            gl::DeleteShader(frag_shader);
        }

        Self {
            shader_program: program,
            quad_vao,
            quad_vbo,
            quad_ibo,
            pos_buf,
            pos_tex,
            width: 0,
            height: 0
        }
    }

    pub fn update_viewport_size(&mut self, width: i32, height: i32) {
        unsafe { gl::Viewport(0, 0, width, height) }
        self.width = width as u32;
        self.height = height as u32;
    }

    pub fn pack_floats(a: f32, b: f32) -> u32 {
        let ha = half::f16::from_f32(a);
        let hb = half::f16::from_f32(b);
        ((hb.to_bits() as u32) << 16) | (ha.to_bits() as u32)
    }

    pub fn render(&self, font: &mut Font, text: &str, chars_per_line: u32) {
        let char_size = 2.0 / chars_per_line as f32;
        let char_size_px = self.width as f32 / chars_per_line as f32;
        let aspect_ratio = self.width as f32 / self.height as f32;

        let face_metrics = font.get_face_metrics();
        let mut offsets: Vec<Offset> = vec!();
        let mut x = 0.0;
        let mut y = face_metrics.height;
        for c in 0..text.len() {
            let c_val = text.as_bytes()[c] as char;
            if c_val.is_whitespace() {
                continue;
            }
            //x = (c % (chars_per_line as usize)) as f32;
            //y = (c / (chars_per_line as usize)) as f32;
            let glyph_metrics = font.get_glyph_data(c_val);
            let texcoord = Font::get_atlas_texcoord(glyph_metrics.atlas_index);
            offsets.push(Offset {
                position: Self::pack_floats(x, y),
                // TODO: scale should depend on glyph size versus actual width
                scale: Self::pack_floats(glyph_metrics.width, glyph_metrics.height),
                tex_coord: Self::pack_floats(texcoord[0], texcoord[1])
            });
            log::warn!("{}", glyph_metrics.advance[0]);
            x += glyph_metrics.advance[0];
            //y = (c / (chars_per_line as usize)) as f32;
        }

        unsafe {
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);

            // Bind program data
            gl::UseProgram(self.shader_program);
            gl::BindVertexArray(self.quad_vao);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.quad_ibo);

            // Update offset data
            gl::BindBuffer(gl::TEXTURE_BUFFER, self.pos_buf);
            gl::BufferSubData(
                gl::TEXTURE_BUFFER,
                0,
                (size_of::<Offset>() * offsets.len()) as isize,
                offsets.as_ptr() as *const c_void
            );

            // Bind position data
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_BUFFER, self.pos_tex);
            gl::TexBuffer(gl::TEXTURE_BUFFER, gl::RGB32F, self.pos_buf);
            gl::Uniform1i(gl::GetUniformLocation(
                self.shader_program,
                b"offsetTex\0".as_ptr() as _
            ), 0);

            // Bind atlas tex
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, font.get_atlas_tex().get_id());
            gl::Uniform1i(gl::GetUniformLocation(
                self.shader_program,
                b"atlasTex\0".as_ptr() as _
            ), 1);

            // Set pixel range
            let pixel_range = char_size_px / font.get_glyph_size() as f32 * font.get_pixel_range();
            gl::Uniform1f(gl::GetUniformLocation(
                self.shader_program,
                b"pixelRange\0".as_ptr() as _
            ), pixel_range);

            // Set texcoord scale
            let coord_scale = font.get_glyph_size() as f32 / font.get_atlas_size() as f32;
            gl::Uniform2fv(gl::GetUniformLocation(
                self.shader_program,
                b"texCoordScale\0".as_ptr() as _
            ), 1, [coord_scale, coord_scale].as_ptr());

            // Set global model matrix (column major)
            let model_mat: [f32; 16] = [
                char_size, 0.0, 0.0, 0.0,
                0.0, char_size * aspect_ratio, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0
            ];
            gl::UniformMatrix4fv(gl::GetUniformLocation(
                self.shader_program,
                "model".as_ptr() as *const i8
            ), 1, gl::FALSE, model_mat.as_ptr());

            gl::DrawElementsInstanced(
                gl::TRIANGLES,
                6,
                gl::UNSIGNED_INT,
                null(),
                offsets.len() as i32
            );

            gl::BindVertexArray(0);
        }
    }

    fn compile_shader(shader_type: u32, source: &[u8]) -> Result<u32, String> {
        unsafe {
            let source_ptr: *const i8 = source.as_ptr() as *const i8;
            let source_ptr_ptr: *const *const i8 = &source_ptr;
            let shader_id: u32 = gl::CreateShader(shader_type);
            gl::ShaderSource(shader_id, 1, source_ptr_ptr, null());
            gl::CompileShader(shader_id);

            let mut success: i32 = 0;
            gl::GetShaderiv(shader_id, gl::COMPILE_STATUS, &mut success);
            if success == 0 {
                let mut log: [i8; 512] = [0; 512];
                gl::GetShaderInfoLog(shader_id, log.len() as i32, null_mut(), log.as_mut_ptr());
                gl::DeleteShader(shader_id);
                Err(String::from_utf8_unchecked(log.iter().map(|&c| c as u8).collect()))
            } else {
                Ok(shader_id)
            }
        }
    }

    fn link_program(vert_shader: u32, frag_shader: u32) -> Result<u32, String> {
        unsafe {
            let program = gl::CreateProgram();
            gl::AttachShader(program, vert_shader);
            gl::AttachShader(program, frag_shader);
            gl::LinkProgram(program);

            let mut success: i32 = 0;
            gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
            if success == 0 {
                let mut log: [i8; 512] = [0; 512];
                gl::GetProgramInfoLog(program, log.len() as i32, null_mut(), log.as_mut_ptr());
                gl::DeleteProgram(program);
                Err(String::from_utf8_unchecked(log.iter().map(|&c| c as u8).collect()))
            } else {
                Ok(program)
            }
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let del_vao: [u32; 1] = [self.quad_vao];
        let del_tex: [u32; 1] = [self.pos_tex];
        let del_buf: [u32; 3] = [self.quad_vbo, self.quad_ibo, self.pos_buf];
        unsafe {
            gl::DeleteVertexArrays(del_vao.len() as i32, del_vao.as_ptr());
            gl::DeleteTextures(del_tex.len() as i32, del_tex.as_ptr());
            gl::DeleteBuffers(del_buf.len() as i32, del_buf.as_ptr());
            gl::DeleteProgram(self.shader_program);
        }
    }
}
