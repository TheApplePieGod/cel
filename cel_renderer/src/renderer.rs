use std::{borrow::{Borrow, BorrowMut}, cell::RefCell, mem::size_of, ptr::{null, null_mut}, rc::Rc};
use std::time::{Duration, SystemTime};
use cel_core::ansi::{CursorStyle, TerminalState};
use crate::{font::{Font, FaceMetrics, RenderType}, util::Util, glchk};

const MAX_CHARACTERS: u32 = 20000;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    pub position: u32,   // x, y
    pub tex_coord: u32,  // u, v
    pub color: [u32; 3]  // r, g, b (fg, bg)
}

pub struct Renderer {
    msdf_program: u32,
    raster_program: u32,
    bg_program: u32,
    quad_vao: u32,
    quad_vbo: u32,
    width: u32,
    height: u32,
    scale: [f32; 2],
    font: Rc<RefCell<Font>>
}

struct RenderConstants {
    aspect_ratio: f32,
    char_root_size: f32, // Fundamental base size of one character cell
    char_size_x_px: f32,
    char_size_y_px: f32,
    char_size_x_screen: f32,
    char_size_y_screen: f32,
    line_height: f32
}

impl Renderer {
    pub fn new(
        width: i32,
        height: i32,
        scale: [f32; 2],
        default_font: Rc<RefCell<Font>>
    ) -> Self {
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

                gl_Position = model * vec4(pos.x, -pos.y, 0.0, 1.0)
                    + vec4(-1.f, 0.f, 0.f, 0.f); // Move origin to top left 
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

        let bg_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;
            in vec3 fgColor;
            in vec3 bgColor;

            out vec4 fragColor;

            void main()
            {
                fragColor = vec4(bgColor, 1.f);
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
        let bg_frag_shader = match Self::compile_shader(gl::FRAGMENT_SHADER, bg_frag_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile bg frag shader: {}", msg)
        };
        let msdf_program = match Self::link_program(vert_shader, msdf_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link msdf program: {}", msg)
        };
        let raster_program = match Self::link_program(vert_shader, raster_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link raster program: {}", msg)
        };
        let bg_program = match Self::link_program(vert_shader, bg_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link bg program: {}", msg)
        };

        // Free shaders
        unsafe {
            gl::DeleteShader(vert_shader);
            gl::DeleteShader(msdf_frag_shader);
            gl::DeleteShader(raster_frag_shader);
            gl::DeleteShader(bg_frag_shader);
        }

        let mut obj = Self {
            msdf_program,
            raster_program,
            bg_program,
            quad_vao,
            quad_vbo,
            width: width as u32,
            height: height as u32,
            scale,
            font: default_font
        };

        // Set initial viewport
        obj.update_viewport_size(width, height);

        obj
    }

    pub fn update_scale(&mut self, scale: [f32; 2]) {
        self.scale = scale;
        self.update_viewport_size(self.width as i32, self.height as i32);
    }

    pub fn update_viewport_size(&mut self, width: i32, height: i32) {
        let scaled_width = (width as f32 * self.scale[0]) as i32;
        let scaled_height = (height as f32 * self.scale[1]) as i32;
        unsafe { gl::Viewport(0, 0, scaled_width, scaled_height) }
        self.width = width as u32;
        self.height = height as u32;
    }

    pub fn compute_max_lines(&self, chars_per_line: u32, screen_height: f32) -> u32 {
        let rc = self.compute_render_constants(chars_per_line);
        let lines_per_screen = (1.0 / (rc.line_height * rc.char_size_y_screen)).floor();

        (lines_per_screen * screen_height) as u32
    }

    /// Returns rendered line count
    pub fn render_terminal(
        &mut self,
        screen_offset: &[f32; 2],
        terminal_state: &TerminalState,
        chars_per_line: u32,
        lines_per_screen: u32,
        line_offset: f32,
        wrap: bool,
        debug_line_number: bool,
        debug_show_cursor: bool
    ) -> u32 {
        // Setup render state
        let base_x = 0.0; //0.25;
        let base_y = 0.0;
        let face_metrics = self.font.as_ref().borrow().get_face_metrics();
        let rc = self.compute_render_constants(chars_per_line);
        let timestamp_seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::new(0, 0))
            .as_secs_f64();

        let mut x = base_x;
        let mut y = base_y - rc.line_height;
        let mut should_render_cursor = match debug_show_cursor {
            true => true,
            false => terminal_state.cursor_state.visible && (
                        !terminal_state.cursor_state.blinking || timestamp_seconds.fract() <= 0.5
                     )
        };
        let mut can_scroll_down = true;
        let mut rendered_line_count = 0;
        let mut max_line_count = 0;
        let mut prev_bg_color = terminal_state.background_color;
        let mut msdf_vertices: Vec<Vertex> = vec![]; // TODO: reuse
        let mut raster_vertices: Vec<Vertex> = vec![];
        let mut bg_vertices: Vec<Vertex> = vec![];

        //
        // Populate vertex buffers
        //

        let wrap_offset = line_offset.fract();
        let line_offset = line_offset as usize;
        'outer: for line_idx in line_offset..(line_offset + lines_per_screen as usize) {
            rendered_line_count += 1;
            x = base_x;
            y += rc.line_height;

            if line_idx >= terminal_state.screen_buffer.len() {
                can_scroll_down = false;
                continue;
            }

            max_line_count = rendered_line_count;

            // Render cursor
            if should_render_cursor {
                let cursor = &terminal_state.global_cursor;
                if cursor[1] == line_idx {
                    // Compute absolute position to account for wraps
                    should_render_cursor = false;
                    let width = match terminal_state.cursor_state.style {
                        CursorStyle::Bar => 0.15,
                        _ => 1.0
                    };
                    let height = match terminal_state.cursor_state.style {
                        CursorStyle::Underline => 0.09,
                        _ => 1.0
                    };
                    let pos_min = [
                        base_x + (cursor[0] % chars_per_line as usize) as f32 * rc.char_root_size,
                        y + rc.line_height * (cursor[0] / chars_per_line as usize) as f32
                    ];
                    Self::push_quad(
                        &[0.0, 0.0, 0.0],
                        &[1.0, 0.0, 0.0],
                        &[0.0, 0.0],
                        &[0.0, 0.0],
                        &pos_min,
                        &[pos_min[0] + rc.char_root_size * width, pos_min[1] + rc.line_height * height],
                        &mut raster_vertices
                    );
                }
            }

            let line = &terminal_state.screen_buffer[line_idx];
            let line_occupancy = line.len() / chars_per_line as usize + 1;
            let mut start_char = 0;
            if line_idx == line_offset {
                // Account for partially visible wrapped first line
                start_char = (line_occupancy as f32 * wrap_offset) as usize * chars_per_line as usize;
            }

            for char_idx in start_char..line.len() {
                if rendered_line_count > lines_per_screen {
                    max_line_count = lines_per_screen;
                    break 'outer;
                }

                let max_x = rc.char_root_size * chars_per_line as f32 - 0.001;
                let should_wrap = wrap && x >= max_x;
                if should_wrap {
                    max_line_count += 1;
                    rendered_line_count += 1;
                    x = base_x;
                    y += rc.line_height;
                }

                let c = line[char_idx];

                // TODO: store in separate buffer?
                let fg_color = c.fg_color.as_ref().unwrap_or(&[1.0, 1.0, 1.0]);
                let bg_color = c.bg_color.as_ref().unwrap_or(&terminal_state.background_color);

                if bg_color[0] != prev_bg_color[0] ||
                   bg_color[1] != prev_bg_color[1] ||
                   bg_color[2] != prev_bg_color[2]
                {
                    // Set the background color.
                    // We do this by comparing with the previously set background color.
                    // If it changes, we need to update all the next cells with this color,
                    // even ones with no text. Thus, we cannot simply rely on the background
                    // of each character. So, we push two quads, one that goes to the end of
                    // the line and one that goes down the rest of the screen. This will optimize
                    // the majority case when background does not change that often. Worst case,
                    // background changes every character and we push 2n background quads.
                    // Unfortunately, we have to check this for every whitespace as well in case the 
                    // background color changes.

                    prev_bg_color = *bg_color;

                    // TODO: we can drastically simplify this because we don't need most of this info

                    // Quad extends to end of line
                    Self::push_quad(
                        fg_color,
                        bg_color,
                        &[0.0, 0.0],
                        &[0.0, 0.0],
                        &[x, y],
                        &[max_x, y + rc.line_height],
                        &mut bg_vertices
                    );

                    // Quad extends fully below, excluding this line
                    // Should probably do some math so y does not go below the screen
                    // but it's fine
                    Self::push_quad(
                        fg_color,
                        bg_color,
                        &[0.0, 0.0],
                        &[0.0, 0.0],
                        &[base_x, y + rc.line_height],
                        &[max_x, y + rc.line_height * lines_per_screen as f32],
                        &mut bg_vertices
                    );
                }

                if c.elem.is_whitespace() || c.elem == '\0' {
                    x += rc.char_root_size;
                    continue;
                }

                let char_to_draw = match debug_line_number {
                    true => char::from_u32((line_idx as u32) % 10 + 48).unwrap(),
                    false => c.elem
                };
                self.push_char_quad(
                    char_to_draw,
                    &face_metrics,
                    fg_color,
                    bg_color,
                    &[x, y],
                    &mut msdf_vertices,
                    &mut raster_vertices
                );

                x += rc.char_root_size;
            }
        }

        self.draw_text_vertices(
            &rc,
            &screen_offset,
            &bg_vertices,
            &msdf_vertices,
            &raster_vertices
        );

        max_line_count
    }

    pub fn draw_quad(
        &self,
        screen_offset: &[f32; 2],
        bg_size_screen: &[f32; 2],
        bg_color: &[f32; 3],
    ) {
        let mut bg_vertices: Vec<Vertex> = vec![];

        // Draw background, separately from text
        let bg_model_mat: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            screen_offset[0], screen_offset[1], 0.0, 1.0
        ];
        Self::push_quad(
            &[0.0, 0.0, 0.0],
            bg_color,
            &[0.0, 0.0],
            &[0.0, 0.0],
            &[0.0, 0.0],
            &bg_size_screen,
            &mut bg_vertices
        );
        self.draw_background_vertices(&bg_vertices, &bg_model_mat);
    }

    // Origin top left to bottom right
    pub fn draw_text(
        &mut self,
        screen_offset: &[f32; 2],
        bg_size_screen: &[f32; 2],
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        text: &str
    ) {
        let face_metrics = self.font.as_ref().borrow().get_face_metrics();
        let rc = self.compute_render_constants(150);

        let mut x = 0.0;
        let mut msdf_vertices: Vec<Vertex> = vec![]; // TODO: reuse
        let mut raster_vertices: Vec<Vertex> = vec![];

        for c in text.chars() {
            if c.is_whitespace() || c == '\0' {
                x += rc.char_root_size;
                continue;
            }

            self.push_char_quad(
                c,
                &face_metrics,
                fg_color,
                bg_color,
                &[x, -rc.line_height],
                &mut msdf_vertices,
                &mut raster_vertices
            );

            x += rc.char_root_size;
        }

        self.draw_quad(screen_offset, bg_size_screen, bg_color);

        // Draw text, centered on background
        let centered_offset = [
            screen_offset[0] + (bg_size_screen[0] - x * rc.char_size_x_screen * 0.5) * 0.5,
            screen_offset[1] - (bg_size_screen[1] - rc.line_height * rc.char_size_y_screen * 0.5) * 0.5,
        ];
        self.draw_text_vertices(
            &rc,
            &centered_offset,
            &vec![],
            &msdf_vertices,
            &raster_vertices
        );
    }

    pub fn get_pixel_width(&self) -> u32 { self.width }
    pub fn get_pixel_height(&self) -> u32 { self.height }
    pub fn get_aspect_ratio(&self) -> f32 { self.width as f32 / self.height as f32 }

    fn compute_render_constants(&self, chars_per_line: u32) -> RenderConstants {
        let face_metrics = self.font.as_ref().borrow().get_face_metrics();
        let aspect_ratio = self.width as f32 / self.height as f32;
        let char_root_size = face_metrics.space_size;
        let char_size_x_px = self.width as f32 / chars_per_line as f32 / char_root_size;
        let char_size_x_screen = char_size_x_px / self.width as f32;

        RenderConstants {
            aspect_ratio,
            char_root_size,
            char_size_x_px,
            char_size_y_px: char_size_x_px * aspect_ratio,
            char_size_x_screen,
            char_size_y_screen: char_size_x_screen * aspect_ratio,
            line_height: 1.0 + face_metrics.descender
        }
    }

    // Min: TL, max: BR
    fn push_quad(
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        uv_min: &[f32; 2],
        uv_max: &[f32; 2],
        pos_min: &[f32; 2],
        pos_max: &[f32; 2],
        vertices: &mut Vec<Vertex>
    ) {
        let color = [
            Util::pack_floats(fg_color[0], bg_color[0]),
            Util::pack_floats(fg_color[1], bg_color[1]),
            Util::pack_floats(fg_color[2], bg_color[2])
        ];

        let tr = Vertex {
            position: Util::pack_floats(pos_max[0], pos_min[1]),
            tex_coord: Util::pack_floats(uv_max[0], uv_min[1]),
            color
        };
        let br = Vertex {
            position: Util::pack_floats(pos_max[0], pos_max[1]),
            tex_coord: Util::pack_floats(uv_max[0], uv_max[1]),
            color
        };
        let bl = Vertex {
            position: Util::pack_floats(pos_min[0], pos_max[1]),
            tex_coord: Util::pack_floats(uv_min[0], uv_max[1]),
            color
        };
        let tl = Vertex {
            position: Util::pack_floats(pos_min[0], pos_min[1]),
            tex_coord: Util::pack_floats(uv_min[0], uv_min[1]),
            color
        };

        vertices.push(tl);
        vertices.push(br);
        vertices.push(tr);
        vertices.push(tl);
        vertices.push(bl);
        vertices.push(br);
    }

    fn push_char_quad(
        &mut self,
        c: char,
        metrics: &FaceMetrics,
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        pos: &[f32; 2], // In character space
        msdf_vertices: &mut Vec<Vertex>,
        raster_vertices: &mut Vec<Vertex>
    ) {
        let mut mut_font = self.font.as_ref().borrow_mut();
        let glyph_metrics = &mut_font.get_glyph_data(c);
        let glyph_bound = &glyph_metrics.glyph_bound;
        let atlas_uv = &glyph_metrics.atlas_uv;

        Self::push_quad(
            fg_color,
            bg_color,
            &[atlas_uv.left, atlas_uv.top],
            &[atlas_uv.right, atlas_uv.bottom],
            &[pos[0] + glyph_bound.left, pos[1] + 1.0 - glyph_bound.top],
            &[pos[0] + glyph_bound.right, pos[1] + 1.0 - glyph_bound.bottom],
            match glyph_metrics.render_type {
                RenderType::MSDF => msdf_vertices,
                RenderType::RASTER => raster_vertices
            }
        );
    }

    fn compute_pixel_range(&self, size_px: f32) -> f32 {
        let font = self.font.as_ref().borrow();
        size_px / font.get_glyph_size() as f32 * font.get_pixel_range()
    }

    fn draw_text_vertices(
        &self,
        rc: &RenderConstants,
        screen_offset: &[f32; 2],
        bg_vertices: &Vec<Vertex>,
        msdf_vertices: &Vec<Vertex>,
        raster_vertices: &Vec<Vertex>,
    ) {
        let pixel_range = self.compute_pixel_range(rc.char_size_x_px);
        let model_mat: [f32; 16] = [
            rc.char_size_x_screen, 0.0, 0.0, 0.0,
            0.0, rc.char_size_y_screen, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            screen_offset[0], screen_offset[1], 0.0, 1.0
        ];

        self.enable_backface_culling();
        if !bg_vertices.is_empty() {
            self.draw_background_vertices(&bg_vertices, &model_mat);
        }
        if !msdf_vertices.is_empty() {
            self.draw_msdf_vertices(&msdf_vertices, &model_mat, pixel_range);
        }
        if !raster_vertices.is_empty() {
            self.draw_raster_vertices(&raster_vertices, &model_mat);
        }
    }

    fn draw_msdf_vertices(
        &self,
        vertices: &[Vertex],
        model: &[f32; 16],
        pixel_range: f32
    ) {
        let font = self.font.as_ref().borrow();

        // Bind program data
        unsafe {
            gl::UseProgram(self.msdf_program);
            gl::BindVertexArray(self.quad_vao);
        }

        // Update vertex data
        self.update_buffer_data(self.quad_vbo, gl::ARRAY_BUFFER, vertices);

        // Bind atlas tex
        self.bind_texture(self.msdf_program, font.get_atlas_tex().get_id(), 0, "atlasTex");

        // Set pixel range
        unsafe {
            gl::Uniform1f(self.get_uniform_location(self.msdf_program, "pixelRange"), pixel_range);
        }

        // Set global model matrix (column major)
        unsafe {
            // Flip translation y
            let mut transformed = model.clone();
            transformed[13] *= -1.0;
            gl::UniformMatrix4fv(
                self.get_uniform_location(self.msdf_program, "model"),
                1, gl::FALSE, transformed.as_ptr()
            );
        }

        self.draw(vertices);
    }

    fn draw_raster_vertices(&self, vertices: &[Vertex], model: &[f32; 16]) {
        let font = self.font.as_ref().borrow();

        // Bind program data
        unsafe {
            gl::UseProgram(self.raster_program);
            gl::BindVertexArray(self.quad_vao);
        }

        // Update vertex data
        self.update_buffer_data(self.quad_vbo, gl::ARRAY_BUFFER, vertices);

        // Bind atlas tex
        self.bind_texture(self.raster_program, font.get_atlas_tex().get_id(), 0, "atlasTex");

        // Set global model matrix (column major)
        unsafe {
            // Flip translation y
            let mut transformed = model.clone();
            transformed[13] *= -1.0;
            gl::UniformMatrix4fv(
                self.get_uniform_location(self.raster_program, "model"),
                1, gl::FALSE, transformed.as_ptr()
            );
        }

        self.draw(vertices);
    }

    fn draw_background_vertices(&self, vertices: &[Vertex], model: &[f32; 16]) {
        // Bind program data
        unsafe {
            gl::UseProgram(self.bg_program);
            gl::BindVertexArray(self.quad_vao);
        }

        // Update vertex data
        self.update_buffer_data(self.quad_vbo, gl::ARRAY_BUFFER, vertices);

        // Set global model matrix (column major)
        unsafe {
            // Flip translation y
            let mut transformed = model.clone();
            transformed[13] *= -1.0;
            gl::UniformMatrix4fv(
                self.get_uniform_location(self.bg_program, "model"),
                1, gl::FALSE, transformed.as_ptr()
            );
        }

        self.draw(vertices);
    }

    fn enable_backface_culling(&self) {
        unsafe {
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
        }
    }

    fn draw<T>(&self, vertices: &[T]) {
        unsafe { gl::DrawArrays(gl::TRIANGLES, 0, vertices.len() as i32) }
    }

    fn bind_buffer(&self, buffer_id: u32, buffer_type: gl::types::GLenum) {
        unsafe { gl::BindBuffer(buffer_type, buffer_id) }
    }

    fn update_buffer_data<T>(
        &self,
        buffer_id: u32,
        buffer_type: gl::types::GLenum,
        data: &[T]
    ) {
        self.bind_buffer(buffer_id, buffer_type);
        unsafe {
            gl::BufferSubData(
                buffer_type, 0,
                (size_of::<T>() * data.len()) as isize,
                data.as_ptr() as _
            );
        }
        self.bind_buffer(0, buffer_type);
    }

    fn get_uniform_location(&self, program_id: u32, name: &str) -> i32 {
        let terminated_string = format!("{name}\0");
        unsafe {
            gl::GetUniformLocation(
                program_id,
                terminated_string.as_ptr() as _
            )
        }
    }

    fn bind_texture(&self, program_id: u32, tex_id: u32, tex_idx: u32, uniform_name: &str) {
        let uniform_location = self.get_uniform_location(program_id, uniform_name);
        unsafe {
            gl::ActiveTexture(gl::TEXTURE0 + tex_idx);
            gl::BindTexture(gl::TEXTURE_2D, tex_id);
            gl::Uniform1i(uniform_location, tex_idx as i32);
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
            gl::DeleteProgram(self.bg_program);
        }
    }
}
