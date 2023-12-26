use std::{ffi::c_void, mem::size_of, ptr::{null, null_mut}, rc::Rc, cell::RefCell};
use crate::{font::{Font, FaceMetrics}, ansi::AnsiHandler};

const MAX_CHARACTERS: u32 = 20000;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    pub position: u32,   // x, y
    pub tex_coord: u32,  // u, v
    pub color: [u32; 3]  // r, g, b (fg, bg)
}

#[derive(Default)]
pub struct RenderState {
    pub font: Option<Rc<RefCell<Font>>>,
    pub wrap: bool,
    pub chars_per_line: u32,
    pub char_size: f32,
    pub char_size_px: f32,
    pub aspect_ratio: f32,
    pub coord_scale: f32,
    pub face_metrics: FaceMetrics,
    pub base_x: f32,
    pub base_y: f32,
    pub msdf_vertices: Vec<Vertex>,
    pub raster_vertices: Vec<Vertex>
}

pub struct Renderer {
    msdf_program: u32,
    raster_program: u32,
    quad_vao: u32,
    quad_vbo: u32,
    width: u32,
    height: u32,
    ansi_handler: AnsiHandler
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
            let coord_stride = size_of::<u32>() as i32 + pos_stride;
            gl::VertexAttribPointer(0, 1, gl::UNSIGNED_INT, gl::FALSE, vert_stride, null());
            gl::VertexAttribPointer(1, 1, gl::UNSIGNED_INT, gl::FALSE, vert_stride, pos_stride as _);
            gl::VertexAttribPointer(2, 3, gl::UNSIGNED_INT, gl::FALSE, vert_stride, coord_stride as _);
            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);
            gl::EnableVertexAttribArray(2);
        }

        let vert_shader_source = b"
            #version 400 core

            layout (location = 0) in uint inPos;
            layout (location = 1) in uint inTexCoord;
            layout (location = 2) in uvec3 inColor;

            out vec2 texCoord;
            out vec3 fgColor;
            out vec3 bgColor;

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
                vec2 r = unpackHalf2x16(inColor.r);
                vec2 g = unpackHalf2x16(inColor.g);
                vec2 b = unpackHalf2x16(inColor.b);

                gl_Position = model * vec4(pos, 0.0, 1.0)
                    + vec4(-1.f, 1.f, 0.f, 0.f); // Move origin to top left 
                texCoord = coord;
                fgColor = vec3(r.x, g.x, b.x);
                bgColor = vec3(r.y, g.y, b.y);
            }
        \0";

        let msdf_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;
            in vec3 fgColor;
            in vec3 bgColor;

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
                
                fragColor = vec4(mix(bgColor, fgColor, opacity), 1.f);
            }
        \0";

        let raster_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;
            in vec3 fgColor;
            in vec3 bgColor;

            out vec4 fragColor;

            uniform sampler2D atlasTex;

            void main()
            {
                vec4 color = texture(atlasTex, texCoord);
                fragColor = vec4(mix(bgColor, color.rgb, color.a), 1.f);
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
            height: 0,
            ansi_handler: AnsiHandler::new()
        }
    }

    pub fn update_viewport_size(&mut self, width: i32, height: i32) {
        unsafe { gl::Viewport(0, 0, width, height) }
        self.width = width as u32;
        self.height = height as u32;
    }

    pub fn render(
        &mut self,
        font: &Rc<RefCell<Font>>,
        text: &Vec<String>,
        chars_per_line: u32,
        line_offset: f32,
        wrap: bool
    ) {
        // Setup render state
        {
            let render_state = self.ansi_handler.get_render_state_mut();
            render_state.font = Some(font.clone());
            render_state.wrap = wrap;
            render_state.chars_per_line = chars_per_line;
            render_state.char_size = 2.0 / chars_per_line as f32;
            render_state.char_size_px = self.width as f32 / chars_per_line as f32;
            render_state.aspect_ratio = self.width as f32 / self.height as f32;
            render_state.coord_scale = 1.0 / font.borrow().get_atlas_size() as f32;
            render_state.face_metrics = font.borrow().get_face_metrics();
            render_state.base_x = 0.25;
            render_state.base_y = -(render_state.face_metrics.height * (-line_offset + 1.0));
            render_state.msdf_vertices.clear();
            render_state.raster_vertices.clear();
        }

        // Parse the input sequence and populate the final render state 
        self.ansi_handler.handle_sequence(&text);

        let render_state = self.ansi_handler.get_render_state();
        let model_mat: [f32; 16] = [
            render_state.char_size, 0.0, 0.0, 0.0,
            0.0, render_state.char_size * render_state.aspect_ratio, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0
        ];

        unsafe {
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
        }

        // Render MSDF
        unsafe {
            let vertices = &render_state.msdf_vertices;

            // Bind program data
            gl::UseProgram(self.msdf_program);
            gl::BindVertexArray(self.quad_vao);

            // Update vertex data
            gl::BindBuffer(gl::ARRAY_BUFFER, self.quad_vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (size_of::<Vertex>() * vertices.len()) as isize,
                vertices.as_ptr() as _
            );

            // Bind atlas tex
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, font.borrow().get_atlas_tex().get_id());
            gl::Uniform1i(gl::GetUniformLocation(
                self.msdf_program,
                b"atlasTex\0".as_ptr() as _
            ), 0);

            // Set pixel range
            let pixel_range = render_state.char_size_px / font.borrow().get_glyph_size() as f32 * font.borrow().get_pixel_range();
            gl::Uniform1f(gl::GetUniformLocation(
                self.msdf_program,
                b"pixelRange\0".as_ptr() as _
            ), pixel_range);

            // Set global model matrix (column major)
            gl::UniformMatrix4fv(gl::GetUniformLocation(
                self.msdf_program,
                "model".as_ptr() as *const i8
            ), 1, gl::FALSE, model_mat.as_ptr());

            gl::DrawArrays(gl::TRIANGLES, 0, vertices.len() as i32);
        }

        // Render 
        unsafe {
            let vertices = &render_state.raster_vertices;

            // Bind program data
            gl::UseProgram(self.raster_program);
            gl::BindVertexArray(self.quad_vao);

            // Update vertex data
            gl::BindBuffer(gl::ARRAY_BUFFER, self.quad_vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (size_of::<Vertex>() * vertices.len()) as isize,
                vertices.as_ptr() as _
            );

            // Bind atlas tex
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, font.borrow().get_atlas_tex().get_id());
            gl::Uniform1i(gl::GetUniformLocation(
                self.raster_program,
                b"atlasTex\0".as_ptr() as _
            ), 0);

            // Set global model matrix (column major)
            gl::UniformMatrix4fv(gl::GetUniformLocation(
                self.raster_program,
                "model".as_ptr() as *const i8
            ), 1, gl::FALSE, model_mat.as_ptr());

            gl::DrawArrays(gl::TRIANGLES, 0, vertices.len() as i32);
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
