use std::{ffi::c_void, mem::size_of, ptr::{null, null_mut}};
use crate::font::{Font, RenderType};

const MAX_CHARACTERS: u32 = 20000;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    position: u32,
    tex_coord: u32
}

pub struct Renderer {
    msdf_program: u32,
    raster_program: u32,
    quad_vao: u32,
    quad_vbo: u32,
    width: u32,
    height: u32
}

impl Renderer {
    pub fn new() -> Self {
        // Generate buffers
        let mut quad_vao: u32 = 0;
        let mut quad_vbo: u32 = 0;
        unsafe {
            // VAO
            gl::GenVertexArrays(1, &mut quad_vao);
            gl::BindVertexArray(quad_vao);

            // VBO
            gl::GenBuffers(1, &mut quad_vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, quad_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (size_of::<Vertex>() * MAX_CHARACTERS as usize * 6) as isize,
                null(),
                gl::DYNAMIC_DRAW
            );

            // VAO attributes
            let vert_stride = size_of::<Vertex>() as i32;
            let pos_stride = size_of::<u32>() as i32;
            gl::VertexAttribPointer(0, 1, gl::UNSIGNED_INT, gl::FALSE, vert_stride, null());
            gl::VertexAttribPointer(1, 1, gl::UNSIGNED_INT, gl::FALSE, vert_stride, pos_stride as *const c_void);
            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);
        }

        let vert_shader_source = b"
            #version 400 core

            layout (location = 0) in uint inPos;
            layout (location = 1) in uint inTexCoord;

            out vec2 texCoord;

            uniform mat4 model;

            uint half2float(uint h) {
                return ((h & uint(0x8000)) << uint(16)) | ((( h & uint(0x7c00)) + uint(0x1c000)) << uint(13)) | ((h & uint(0x03ff)) << uint(13));
            }

            vec2 unpackHalf2x16(uint v) {	
                return vec2(uintBitsToFloat(half2float(v & uint(0xffff))),
                        uintBitsToFloat(half2float(v >> uint(16))));
            }

            void main()
            {
                vec2 pos = unpackHalf2x16(inPos);
                vec2 coord = unpackHalf2x16(inTexCoord);

                gl_Position = model * vec4(pos, 0.0, 1.0)
                    + vec4(-1.f, 1.f, 0.f, 0.f); // Move origin to top left 
                texCoord = coord;
            }
        \0";

        let msdf_frag_shader_source = b"
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
                
                //fragColor = mix(vec4(1.f, 0.f, 0.f, 1.f), vec4(1.f), opacity);
                fragColor = mix(vec4(0.f), vec4(1.f), opacity);

                /*
                if (opacity < 0.005)
                    discard;
                fragColor = vec4(1.f, 1.f, 1.f, opacity);
                */

                //fragColor = msd;
                //fragColor = vec4(opacity, opacity, opacity, 1.f);
                //fragColor = vec4(texCoord.r, texCoord.g, 0.f, 1.f);
            }
        \0";

        let raster_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;

            out vec4 fragColor;

            uniform sampler2D atlasTex;

            void main()
            {
                vec4 color = texture(atlasTex, texCoord);
                fragColor = mix(vec4(0.f), color, color.a);
            }
        \0";

        // Compile shaders & generate program
        let vert_shader = match Self::compile_shader(gl::VERTEX_SHADER, vert_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile vertex shader: {}", msg)
        };
        let msdf_frag_shader = match Self::compile_shader(gl::FRAGMENT_SHADER, msdf_frag_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile msdf frag shader: {}", msg)
        };
        let raster_frag_shader = match Self::compile_shader(gl::FRAGMENT_SHADER, raster_frag_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile raster frag shader: {}", msg)
        };
        let msdf_program = match Self::link_program(vert_shader, msdf_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link msdf program: {}", msg)
        };
        let raster_program = match Self::link_program(vert_shader, raster_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link raster program: {}", msg)
        };

        // Free shaders
        unsafe {
            gl::DeleteShader(vert_shader);
            gl::DeleteShader(msdf_frag_shader);
            gl::DeleteShader(raster_frag_shader);
        }

        Self {
            msdf_program,
            raster_program,
            quad_vao,
            quad_vbo,
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

    pub fn render(
        &self,
        font: &mut Font,
        text: &Vec<String>,
        chars_per_line: u32,
        line_offset: f32,
        wrap: bool
    ) {
        let char_size = 2.0 / chars_per_line as f32;
        let char_size_px = self.width as f32 / chars_per_line as f32;
        let aspect_ratio = self.width as f32 / self.height as f32;
        let coord_scale = 1.0 / font.get_atlas_size() as f32;

        let face_metrics = font.get_face_metrics();
        let base_x = 0.25;
        let mut msdf_vertices: Vec<Vertex> = vec!();
        let mut raster_vertices: Vec<Vertex> = vec!();
        let mut x = base_x;
        let mut y = -(face_metrics.height * (-line_offset + 1.0));
        for batch in text {
            for c in batch.chars() {
                if c.is_whitespace() {
                    match c {
                        ' ' => x += face_metrics.space_size,
                        '\t' => x += face_metrics.space_size * font.get_tab_width(),
                        '\n' => {
                            x = base_x;
                            y -= face_metrics.height;
                        }
                        _ => {}
                    }

                    continue;
                } else if c.is_control() {
                    continue;
                }

                if wrap && x >= chars_per_line as f32 - 1.0 {
                    x = base_x;
                    y -= face_metrics.height;
                }

                //TODO: precompute in metrics
                // UV.y is flipped since the underlying atlas bitmaps have flipped y
                let glyph_metrics = &font.get_glyph_data(c);
                let glyph_bound = &glyph_metrics.glyph_bound;
                let atlas_bound = &glyph_metrics.atlas_bound;
                let uv = Font::get_atlas_texcoord(glyph_metrics.atlas_index);
                let uv_min = [
                    uv[0] + atlas_bound.left * coord_scale,
                    uv[1] + atlas_bound.top * coord_scale
                ];
                let uv_max = [
                    uv_min[0] + atlas_bound.width() * coord_scale,
                    uv_min[1] - atlas_bound.height() * coord_scale
                ];

                let tr = Vertex {
                    position: Self::pack_floats(x + glyph_bound.right, y + glyph_bound.top),
                    tex_coord: Self::pack_floats(uv_max[0], uv_min[1])
                };
                let br = Vertex {
                    position: Self::pack_floats(x + glyph_bound.right, y + glyph_bound.bottom),
                    tex_coord: Self::pack_floats(uv_max[0], uv_max[1])
                };
                let bl = Vertex {
                    position: Self::pack_floats(x + glyph_bound.left, y + glyph_bound.bottom),
                    tex_coord: Self::pack_floats(uv_min[0], uv_max[1])
                };
                let tl = Vertex {
                    position: Self::pack_floats(x + glyph_bound.left, y + glyph_bound.top),
                    tex_coord: Self::pack_floats(uv_min[0], uv_min[1])
                };

                let push_vec = match glyph_metrics.render_type {
                    RenderType::MSDF => &mut msdf_vertices,
                    RenderType::RASTER => &mut raster_vertices
                };

                push_vec.push(tl);
                push_vec.push(br);
                push_vec.push(tr);
                push_vec.push(tl);
                push_vec.push(bl);
                push_vec.push(br);

                x += glyph_metrics.advance;
            }
        }

        let model_mat: [f32; 16] = [
            char_size, 0.0, 0.0, 0.0,
            0.0, char_size * aspect_ratio, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0
        ];

        unsafe {
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
        }

        // Render MSDF
        unsafe {
            // Bind program data
            gl::UseProgram(self.msdf_program);
            gl::BindVertexArray(self.quad_vao);

            // Update vertex data
            gl::BindBuffer(gl::ARRAY_BUFFER, self.quad_vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (size_of::<Vertex>() * msdf_vertices.len()) as isize,
                msdf_vertices.as_ptr() as _
            );

            // Bind atlas tex
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, font.get_atlas_tex().get_id());
            gl::Uniform1i(gl::GetUniformLocation(
                self.msdf_program,
                b"atlasTex\0".as_ptr() as _
            ), 0);

            // Set pixel range
            let pixel_range = char_size_px / font.get_glyph_size() as f32 * font.get_pixel_range();
            gl::Uniform1f(gl::GetUniformLocation(
                self.msdf_program,
                b"pixelRange\0".as_ptr() as _
            ), pixel_range);

            // Set global model matrix (column major)
            gl::UniformMatrix4fv(gl::GetUniformLocation(
                self.msdf_program,
                "model".as_ptr() as *const i8
            ), 1, gl::FALSE, model_mat.as_ptr());

            gl::DrawArrays(gl::TRIANGLES, 0, msdf_vertices.len() as i32);
        }

        // Render 
        unsafe {
            // Bind program data
            gl::UseProgram(self.raster_program);
            gl::BindVertexArray(self.quad_vao);

            // Update vertex data
            gl::BindBuffer(gl::ARRAY_BUFFER, self.quad_vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (size_of::<Vertex>() * raster_vertices.len()) as isize,
                raster_vertices.as_ptr() as _
            );

            // Bind atlas tex
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, font.get_atlas_tex().get_id());
            gl::Uniform1i(gl::GetUniformLocation(
                self.raster_program,
                b"atlasTex\0".as_ptr() as _
            ), 0);

            // Set global model matrix (column major)
            gl::UniformMatrix4fv(gl::GetUniformLocation(
                self.raster_program,
                "model".as_ptr() as *const i8
            ), 1, gl::FALSE, model_mat.as_ptr());

            gl::DrawArrays(gl::TRIANGLES, 0, raster_vertices.len() as i32);
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
        let del_buf: [u32; 1] = [self.quad_vbo];
        unsafe {
            gl::DeleteVertexArrays(del_vao.len() as i32, del_vao.as_ptr());
            gl::DeleteBuffers(del_buf.len() as i32, del_buf.as_ptr());
            gl::DeleteProgram(self.msdf_program);
            gl::DeleteProgram(self.raster_program);
        }
    }
}
