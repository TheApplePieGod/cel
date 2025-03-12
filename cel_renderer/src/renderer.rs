use cel_core::ansi::{CellContent, CursorStyle, StyleFlags, TerminalState};
use std::time::{Duration, SystemTime};
use std::{
    cell::RefCell,
    mem::size_of,
    ptr::{null, null_mut},
    rc::Rc,
};

use crate::{
    font::{FaceMetrics, Font, RenderType},
    glchk,
    util::Util,
};

const MAX_CHARACTERS: u32 = 50000;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct QuadData {
    pub position: [f32; 4],  // x0, y0, x1, y1
    pub tex_coord: [f32; 4], // u0, v0, u1, v1
    pub color: [u32; 3],     // r, g, b (fg, bg)
    pub flags: StyleFlags
}

pub struct Renderer {
    msdf_program: u32,
    raster_program: u32,
    bg_program: u32,
    quad_vao: u32,
    quad_ibo: u32,
    instance_vbo: u32,
    width: u32,
    height: u32,
    scale: [f32; 2],
    font: Rc<RefCell<Font>>,
}

pub struct RenderConstants {
    pub char_root_size: f32, // Fundamental base size of one character cell
    pub char_size_x_px: f32,
    pub char_size_y_px: f32,
    pub char_size_x_screen: f32,
    pub char_size_y_screen: f32,
    pub line_height: f32,
}

impl Renderer {
    pub fn new(width: i32, height: i32, scale: [f32; 2], default_font: Rc<RefCell<Font>>) -> Self {
        // Generate buffers
        let mut quad_vao: u32 = 0;
        let mut quad_ibo: u32 = 0;
        let mut instance_vbo: u32 = 0;
        unsafe {
            // VAO
            gl::GenVertexArrays(1, &mut quad_vao);
            gl::BindVertexArray(quad_vao);

            let base_indices: [u32; 6] = [0, 2, 3, 0, 1, 2];

            // Main IBO
            gl::GenBuffers(1, &mut quad_ibo);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, quad_ibo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (size_of::<u32>() * base_indices.len()) as isize,
                base_indices.as_ptr() as _,
                gl::STATIC_DRAW,
            );

            // Instance VBO
            gl::GenBuffers(1, &mut instance_vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, instance_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (size_of::<QuadData>() * MAX_CHARACTERS as usize) as isize,
                null(),
                gl::DYNAMIC_DRAW,
            );

            // VAO attributes
            let instance_stride = size_of::<QuadData>() as i32;
            let pos_stride = size_of::<f32>() as i32 * 4;
            let coord_stride = size_of::<f32>() as i32 * 4 + pos_stride;
            let color_stride = size_of::<u32>() as i32 * 3 + coord_stride;
            gl::VertexAttribPointer(0, 4, gl::FLOAT, gl::FALSE, instance_stride, null());
            gl::VertexAttribPointer(1, 4, gl::FLOAT, gl::FALSE, instance_stride, pos_stride as _);
            gl::VertexAttribPointer(2, 3, gl::UNSIGNED_INT, gl::FALSE, instance_stride, coord_stride as _);
            gl::VertexAttribPointer(3, 1, gl::UNSIGNED_INT, gl::FALSE, instance_stride, color_stride as _);
            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);
            gl::EnableVertexAttribArray(2);
            gl::EnableVertexAttribArray(3);
            gl::VertexAttribDivisor(0, 1);
            gl::VertexAttribDivisor(1, 1);
            gl::VertexAttribDivisor(2, 1);
            gl::VertexAttribDivisor(3, 1);
        }

        let vert_shader_source = b"
            #version 400 core

            layout (location = 0) in vec4 inPos;
            layout (location = 1) in vec4 inTexCoord;
            layout (location = 2) in uvec3 inColor;
            layout (location = 3) in uint inFlags;

            out vec2 texCoord;
            out vec3 fgColor;
            out vec3 bgColor;
            flat out uint flags;

            uniform mat4 model;
            uniform vec2 scale;

            uint half2float(uint h) {
                return ((h & uint(0x8000)) << uint(16)) | ((( h & uint(0x7c00)) + uint(0x1c000)) << uint(13)) | ((h & uint(0x03ff)) << uint(13));
            }

            vec2 unpackHalf2x16(uint v) {	
                return vec2(uintBitsToFloat(half2float(v & uint(0xffff))),
                        uintBitsToFloat(half2float(v >> uint(16))));
            }

            const vec2 offsets[4] = vec2[](
                vec2(0.0, 0.0), // Bottom-left
                vec2(1.0, 0.0), // Bottom-right
                vec2(1.0, 1.0), // Top-right
                vec2(0.0, 1.0)  // Top-left
            );

            void main()
            {
                // Extract quad corners
                vec2 p0 = inPos.xy;
                vec2 p1 = inPos.zw;

                // Compute vertex position based on gl_VertexID
                vec2 offset = offsets[gl_VertexID % 4];
                vec2 pos = mix(p0, p1, offset);

                // Apply shear transformation
                float shearAmount = (inFlags & 8U) * 0.015f;
                pos.x += offset.y * shearAmount;

                // Compute texture coordinates
                vec2 tex0 = inTexCoord.xy;
                vec2 tex1 = inTexCoord.zw;
                vec2 coord = mix(tex0, tex1, offset);

                vec2 r = unpackHalf2x16(inColor.r);
                vec2 g = unpackHalf2x16(inColor.g);
                vec2 b = unpackHalf2x16(inColor.b);

                // TODO: make this a uniform
                mat4 scalingMat = mat4(
                    scale[0], 0.0, 0.0, 0.0,
                    0.0, -scale[1], 0.0, 0.0,
                    0.0, 0.0, 1.0, 0.0,
                    0.0, 0.0, 0.0, 1.0
                );

                gl_Position = scalingMat * model * vec4(pos, 0.0, 1.0)
                    + vec4(-1.f, 1.f, 0.f, 0.f); // Move origin to top left 
                texCoord = coord;
                fgColor = vec3(r.x, g.x, b.x);
                bgColor = vec3(r.y, g.y, b.y);
                flags = inFlags;
            }
        \0";

        let msdf_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;
            in vec3 fgColor;
            in vec3 bgColor;
            flat in uint flags;

            out vec4 fragColor;

            uniform sampler2D atlasTex;
            uniform float pixelRange;

            float Median(float r, float g, float b, float a) {
                return max(min(r, g), min(max(r, g), b));
            }

            void main()
            {
                float sdFactor = 1.05 + (flags & 1U) * 0.15 - (flags & 2U) * 0.05;
                vec4 msd = texture(atlasTex, texCoord);
                float sd = Median(msd.r, msd.g, msd.b, msd.a) * sdFactor;
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
            Err(msg) => panic!("Failed to compile vertex shader: {}", msg),
        };
        let msdf_frag_shader =
            match Self::compile_shader(gl::FRAGMENT_SHADER, msdf_frag_shader_source) {
                Ok(id) => id,
                Err(msg) => panic!("Failed to compile msdf frag shader: {}", msg),
            };
        let raster_frag_shader =
            match Self::compile_shader(gl::FRAGMENT_SHADER, raster_frag_shader_source) {
                Ok(id) => id,
                Err(msg) => panic!("Failed to compile raster frag shader: {}", msg),
            };
        let bg_frag_shader = match Self::compile_shader(gl::FRAGMENT_SHADER, bg_frag_shader_source)
        {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile bg frag shader: {}", msg),
        };
        let msdf_program = match Self::link_program(vert_shader, msdf_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link msdf program: {}", msg),
        };
        let raster_program = match Self::link_program(vert_shader, raster_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link raster program: {}", msg),
        };
        let bg_program = match Self::link_program(vert_shader, bg_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link bg program: {}", msg),
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
            quad_ibo,
            instance_vbo,
            width: width as u32,
            height: height as u32,
            scale,
            font: default_font,
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

    pub fn compute_max_lines(&self, rc: &RenderConstants, screen_height: f32) -> u32 {
        let lines_per_screen = (1.0 / (rc.line_height * rc.char_size_y_screen)).floor();

        (lines_per_screen * screen_height) as u32
    }

    /// Returns rendered line count
    pub fn render_terminal(
        &mut self,
        terminal_state: &TerminalState,
        screen_offset: &[f32; 2],
        padding_px: &[f32; 2],
        chars_per_line: u32,
        lines_per_screen: u32,
        line_offset: f32,
        wrap: bool,
        debug_line_number: bool,
        debug_col_number: bool,
        debug_show_cursor: bool,
    ) -> u32 {
        // Setup render state
        let rc = self.compute_render_constants(chars_per_line, padding_px);
        let timestamp_seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::new(0, 0))
            .as_secs_f64();
        // Clamp base position to nearest pixel
        let base_x = ((screen_offset[0] / rc.char_size_x_screen) * rc.char_size_x_px).floor() / rc.char_size_x_px;
        let base_y = ((screen_offset[1] / rc.char_size_y_screen) * rc.char_size_y_px).floor() / rc.char_size_y_px;
        let mut x = base_x;
        let mut y = base_y - rc.line_height;
        let mut should_render_cursor = debug_show_cursor
            || (terminal_state.cursor_state.visible
                && (!terminal_state.cursor_state.blinking || timestamp_seconds.fract() <= 0.5));
        let mut can_scroll_down = true;
        let mut rendered_line_count = 0;
        let mut max_line_count = 0;
        let mut msdf_quads: Vec<QuadData> = vec![]; // TODO: reuse
        let mut raster_quads: Vec<QuadData> = vec![];
        let mut bg_quads: Vec<QuadData> = vec![];

        //
        // Populate vertex buffers
        //

        let wrap_offset = line_offset.fract();
        let line_offset = line_offset as usize;
        'outer: for line_idx in line_offset..(line_offset + lines_per_screen as usize) {
            rendered_line_count += 1;
            x = base_x;
            y += rc.line_height;

            let max_offscreen_lines = 10.0;
            let y_pos_screen = y * rc.char_size_y_screen;
            if y_pos_screen < 0.0 - rc.char_size_y_screen * max_offscreen_lines {
                // Account for the size of wrapped offscreen lines
                if line_idx < terminal_state.screen_buffer.len() {
                    let line = &terminal_state.screen_buffer[line_idx];
                    let line_occupancy = (line.len() as u32 / chars_per_line) as u32;
                    rendered_line_count += line_occupancy;
                    y += rc.line_height * line_occupancy as f32;
                } else {
                    break;
                }
                max_line_count = rendered_line_count;
                continue;
            }
            if y_pos_screen > 1.0 + rc.char_size_y_screen * max_offscreen_lines {
                if line_idx >= terminal_state.screen_buffer.len() {
                    break;
                }

                // Account for the size of wrapped offscreen lines
                let line = &terminal_state.screen_buffer[line_idx];
                let line_occupancy = (line.len() as u32 / chars_per_line) as u32;
                rendered_line_count += line_occupancy;
                y += rc.line_height * line_occupancy as f32;

                // TODO: should break here, but the rendered line count gets messed
                // up which breaks other things
                max_line_count = rendered_line_count;
                continue;
            }

            // Render cursor
            if should_render_cursor {
                let cursor = &terminal_state.global_cursor;
                if cursor[1] == line_idx {
                    // Compute absolute position to account for wraps
                    should_render_cursor = false;
                    let width = match terminal_state.cursor_state.style {
                        CursorStyle::Bar => 0.15,
                        _ => 1.0,
                    };
                    let height = match terminal_state.cursor_state.style {
                        CursorStyle::Underline => 0.09,
                        _ => 1.0,
                    };
                    let pos_min = [
                        base_x + (cursor[0] % chars_per_line as usize) as f32 * rc.char_root_size,
                        y + rc.line_height * (cursor[0] / chars_per_line as usize) as f32,
                    ];
                    Self::push_quad(
                        &[0.0, 0.0, 0.0],
                        &[1.0, 0.0, 0.0],
                        &[0.0, 0.0],
                        &[0.0, 0.0],
                        &pos_min,
                        &[
                            pos_min[0] + rc.char_root_size * width,
                            pos_min[1] + rc.line_height * height,
                        ],
                        StyleFlags::default(),
                        &mut raster_quads,
                    );
                }
            }

            if line_idx >= terminal_state.screen_buffer.len() {
                can_scroll_down = false;
                continue;
            }

            max_line_count = rendered_line_count;

            let line = &terminal_state.screen_buffer[line_idx];
            let line_occupancy = line.len() / chars_per_line as usize + 1;
            let mut start_char = 0;
            if line_idx == line_offset {
                // Account for partially visible wrapped first line
                start_char =
                    (line_occupancy as f32 * wrap_offset) as usize * chars_per_line as usize;
            }

            // Store bg color per line for optimization
            let mut prev_bg_color = terminal_state.background_color;

            for char_idx in start_char..line.len() {
                if rendered_line_count > lines_per_screen {
                    max_line_count = lines_per_screen;
                    break 'outer;
                }

                let max_x = base_x + rc.char_root_size * chars_per_line as f32 - 0.001;
                let should_wrap = wrap && x >= max_x;
                if should_wrap {
                    max_line_count += 1;
                    rendered_line_count += 1;
                    x = base_x;
                    y += rc.line_height;
                    prev_bg_color = terminal_state.background_color;
                }

                let elem = &line[char_idx];

                // TODO: store in separate buffer?
                let fg_color = elem.style.fg_color.as_ref().unwrap_or(&[1.0, 1.0, 1.0]);
                let bg_color = elem
                    .style
                    .bg_color
                    .as_ref()
                    .unwrap_or(&terminal_state.background_color);
                if bg_color[0] != prev_bg_color[0]
                    || bg_color[1] != prev_bg_color[1]
                    || bg_color[2] != prev_bg_color[2]
                {
                    // Set the background color.
                    // We do this by comparing with the previously set background color.
                    // If it changes, push a new quad. Otherwiwse, we can extend the previous
                    // quad to save vertices.

                    prev_bg_color = *bg_color;

                    Self::push_quad(
                        fg_color,
                        bg_color,
                        &[0.0, 0.0],
                        &[0.0, 0.0],
                        &[x, y],
                        &[x + rc.char_root_size, y + rc.line_height],
                        StyleFlags::default(),
                        &mut bg_quads,
                    );
                } else if elem.style.bg_color.is_some() {
                    Self::extend_previous_quad(x + rc.char_root_size, &mut bg_quads);
                }

                let mut char_to_draw = None;
                let mut skip = false;
                match &elem.elem {
                    CellContent::Char(c) => {
                        // Skip rendering if this is a whitespace char
                        if c.is_whitespace() || *c == '\0' {
                            skip = true;
                        } else {
                            char_to_draw = Some(*c)
                        }
                    },
                    CellContent::Grapheme(str, len) => {
                        self.push_unicode_quad(
                            str,
                            &rc,
                            fg_color,
                            bg_color,
                            &[x, y],
                            elem.style.flags,
                            &mut msdf_quads,
                            &mut raster_quads,
                        );
                    },
                    CellContent::Continuation(_) => skip = true,
                    CellContent::Empty => skip = true
                };

                if skip {
                    x += rc.char_root_size;
                    continue;
                }

                if debug_line_number || debug_col_number {
                    char_to_draw = Some(if debug_line_number {
                        char::from_u32((line_idx as u32) % 10 + 48).unwrap()
                    } else {
                        char::from_u32((char_idx as u32) % 10 + 48).unwrap()
                    });
                }

                if let Some(char_to_draw) = char_to_draw {
                    self.push_char_quad(
                        char_to_draw,
                        &rc,
                        fg_color,
                        bg_color,
                        &[x, y],
                        elem.style.flags,
                        &mut msdf_quads,
                        &mut raster_quads,
                    );
                }

                x += rc.char_root_size;
            }
        }

        self.draw_text_quads(&rc, &[0.0, 0.0], &bg_quads, &msdf_quads, &raster_quads);

        max_line_count
    }

    pub fn draw_quad(
        &self,
        screen_offset: &[f32; 2],
        bg_size_screen: &[f32; 2],
        bg_color: &[f32; 3],
    ) {
        let mut bg_quads: Vec<QuadData> = vec![];

        // Draw background, separately from text
        let bg_model_mat: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            screen_offset[0], screen_offset[1], 0.0, 1.0,
        ];
        Self::push_quad(
            &[0.0, 0.0, 0.0],
            bg_color,
            &[0.0, 0.0],
            &[0.0, 0.0],
            &[0.0, 0.0],
            &bg_size_screen,
            StyleFlags::default(),
            &mut bg_quads,
        );
        self.draw_background_quads(&bg_quads, &bg_model_mat);
    }

    // Origin top left to bottom right
    pub fn draw_text(
        &mut self,
        chars_per_line: u32,
        screen_offset: &[f32; 2],
        bg_size_screen: &[f32; 2],
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        centered: bool,
        text: &str,
    ) {
        let rc = self.compute_render_constants(chars_per_line, &[0.0, 0.0]);

        let mut x = 0.0;
        let mut y = 0.0;
        let mut msdf_quads: Vec<QuadData> = vec![]; // TODO: reuse
        let mut raster_quads: Vec<QuadData> = vec![];

        for c in text.chars() {
            if c == '\n' {
                x = 0.0;
                y += rc.line_height;
                continue;
            }

            if c.is_whitespace() || c == '\0' {
                x += rc.char_root_size;
                continue;
            }

            self.push_char_quad(
                c,
                &rc,
                fg_color,
                bg_color,
                &[x, y],
                StyleFlags::default(),
                &mut msdf_quads,
                &mut raster_quads,
            );

            x += rc.char_root_size;
        }

        self.draw_quad(screen_offset, bg_size_screen, bg_color);

        // Draw text, centered on background
        let centered_offset = [
            screen_offset[0] + (bg_size_screen[0] - x * rc.char_size_x_screen) * 0.5,
            screen_offset[1] + (bg_size_screen[1] - rc.line_height * rc.char_size_y_screen) * 0.5,
        ];
        self.draw_text_quads(
            &rc,
            match centered {
                true => &centered_offset,
                false => &screen_offset,
            },
            &vec![],
            &msdf_quads,
            &raster_quads,
        );
    }

    pub fn compute_render_constants(
        &self,
        chars_per_line: u32,
        padding_px: &[f32; 2],
    ) -> RenderConstants {
        let real_width = self.width as f32 - padding_px[0] * 2.0;
        let face_metrics = self.font.as_ref().borrow().get_face_metrics();
        let char_size_px = (real_width / chars_per_line as f32 / face_metrics.width).floor();
        let char_size_x_screen = char_size_px / self.width as f32;
        let char_size_y_screen = char_size_px / self.height as f32;

        // Ensure all sizes are pixel-aligned
        RenderConstants {
            char_root_size: (face_metrics.width * char_size_px).ceil() / char_size_px,
            char_size_x_px: char_size_px,
            char_size_y_px: char_size_px,
            char_size_x_screen,
            char_size_y_screen,
            line_height: ((1.0 + face_metrics.descender) * char_size_px).ceil() / char_size_px,
        }
    }

    pub fn get_width(&self) -> u32 {
        self.width
    }
    pub fn get_height(&self) -> u32 {
        self.height
    }
    pub fn get_aspect_ratio(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    fn extend_previous_quad(new_x: f32, quads: &mut Vec<QuadData>) {
        match quads.last_mut() {
            Some(quad) => quad.position[2] = new_x,
            None => {}
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
        flags: StyleFlags,
        arr: &mut Vec<QuadData>,
    ) {
        let color = [
            Util::pack_floats(fg_color[0], bg_color[0]),
            Util::pack_floats(fg_color[1], bg_color[1]),
            Util::pack_floats(fg_color[2], bg_color[2]),
        ];

        arr.push(QuadData {
            position: [pos_min[0], pos_max[1], pos_max[0], pos_min[1]],
            tex_coord: [uv_min[0], uv_max[1], uv_max[0], uv_min[1]],
            color,
            flags
        });
    }

    fn push_char_quad(
        &mut self,
        c: char,
        rc: &RenderConstants,
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        pos: &[f32; 2], // In character space
        flags: StyleFlags,
        msdf_arr: &mut Vec<QuadData>,
        raster_arr: &mut Vec<QuadData>,
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
            &[
                pos[0] + glyph_bound.right,
                pos[1] + 1.0 - glyph_bound.bottom,
            ],
            flags,
            match glyph_metrics.render_type {
                RenderType::MSDF => msdf_arr,
                RenderType::RASTER => raster_arr,
            },
        );
    }

    fn push_unicode_quad(
        &mut self,
        str: &str,
        rc: &RenderConstants,
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        pos: &[f32; 2], // In character space
        flags: StyleFlags,
        msdf_arr: &mut Vec<QuadData>,
        raster_arr: &mut Vec<QuadData>,
    ) {
        let mut mut_font = self.font.as_ref().borrow_mut();
        for metrics in mut_font.get_grapheme_data(str).iter() {
            let glyph_bound = &metrics.glyph_bound;
            let atlas_uv = &metrics.atlas_uv;

            Self::push_quad(
                fg_color,
                bg_color,
                &[atlas_uv.left, atlas_uv.top],
                &[atlas_uv.right, atlas_uv.bottom],
                &[pos[0] + glyph_bound.left, pos[1] + 1.0 - glyph_bound.top],
                &[
                    pos[0] + glyph_bound.right,
                    pos[1] + 1.0 - glyph_bound.bottom,
                ],
                flags,
                match metrics.render_type {
                    RenderType::MSDF => msdf_arr,
                    RenderType::RASTER => raster_arr,
                },
            );
        }
    }

    fn compute_pixel_range(&self, size_px: f32) -> f32 {
        let font = self.font.as_ref().borrow();
        let max_scale = self.scale[0].max(self.scale[1]);
        size_px * max_scale / font.get_glyph_size() as f32 * font.get_pixel_range()
    }

    fn draw_text_quads(
        &self,
        rc: &RenderConstants,
        screen_offset: &[f32; 2],
        bg_quads: &Vec<QuadData>,
        msdf_quads: &Vec<QuadData>,
        raster_quads: &Vec<QuadData>,
    ) {
        let pixel_range = self.compute_pixel_range(rc.char_size_x_px);
        let model_mat: [f32; 16] = [
            rc.char_size_x_screen, 0.0, 0.0, 0.0,
            0.0, rc.char_size_y_screen, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            screen_offset[0], screen_offset[1], 0.0, 1.0,
        ];

        self.enable_backface_culling();
        if !bg_quads.is_empty() {
            self.draw_background_quads(&bg_quads, &model_mat);
        }
        if !msdf_quads.is_empty() {
            self.draw_msdf_quads(&msdf_quads, &model_mat, pixel_range);
        }
        if !raster_quads.is_empty() {
            self.draw_raster_quads(&raster_quads, &model_mat);
        }
    }

    fn draw_msdf_quads(&self, arr: &[QuadData], model: &[f32; 16], pixel_range: f32) {
        let font = self.font.as_ref().borrow();

        // Bind program data
        unsafe {
            gl::UseProgram(self.msdf_program);
            gl::BindVertexArray(self.quad_vao);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.quad_ibo);
        }

        // Update instance data
        self.update_buffer_data(self.instance_vbo, gl::ARRAY_BUFFER, arr);

        // Bind atlas tex
        self.bind_texture(
            self.msdf_program,
            font.get_atlas_tex().get_id(),
            0,
            "atlasTex",
        );

        // Set pixel range
        unsafe {
            gl::Uniform1f(
                self.get_uniform_location(self.msdf_program, "pixelRange"),
                pixel_range,
            );
        }

        self.bind_vertex_shader_data(self.msdf_program, model);

        self.draw_indexed_instanced(arr.len() as i32);
    }

    fn draw_raster_quads(&self, arr: &[QuadData], model: &[f32; 16]) {
        let font = self.font.as_ref().borrow();

        // Bind program data
        unsafe {
            gl::UseProgram(self.raster_program);
            gl::BindVertexArray(self.quad_vao);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.quad_ibo);
        }

        // Update instance data
        self.update_buffer_data(self.instance_vbo, gl::ARRAY_BUFFER, arr);

        // Bind atlas tex
        self.bind_texture(
            self.raster_program,
            font.get_atlas_tex().get_id(),
            0,
            "atlasTex",
        );

        self.bind_vertex_shader_data(self.raster_program, model);

        self.draw_indexed_instanced(arr.len() as i32);
    }

    fn draw_background_quads(&self, arr: &[QuadData], model: &[f32; 16]) {
        // Bind program data
        unsafe {
            gl::UseProgram(self.bg_program);
            gl::BindVertexArray(self.quad_vao);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.quad_ibo);
        }

        // Update instance data
        self.update_buffer_data(self.instance_vbo, gl::ARRAY_BUFFER, arr);

        self.bind_vertex_shader_data(self.bg_program, model);

        self.draw_indexed_instanced(arr.len() as i32);
    }

    fn bind_vertex_shader_data(&self, program_id: u32, model: &[f32; 16]) {
        // Set global model matrix (column major)
        unsafe {
            gl::UniformMatrix4fv(
                self.get_uniform_location(program_id, "model"),
                1,
                gl::FALSE,
                model.as_ptr(),
            );
        }

        // Set content scale
        unsafe {
            gl::Uniform2fv(
                self.get_uniform_location(program_id, "scale"),
                1,
                self.scale.as_ptr(),
            );
        }
    }

    fn enable_backface_culling(&self) {
        unsafe {
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
        }
    }

    fn draw_indexed_instanced(&self, instance_count: i32) {
        unsafe {
            gl::DrawElementsInstanced(gl::TRIANGLES, 6, gl::UNSIGNED_INT, null(), instance_count)
        }
    }

    fn bind_buffer(&self, buffer_id: u32, buffer_type: gl::types::GLenum) {
        unsafe { gl::BindBuffer(buffer_type, buffer_id) }
    }

    fn update_buffer_data<T>(&self, buffer_id: u32, buffer_type: gl::types::GLenum, data: &[T]) {
        self.bind_buffer(buffer_id, buffer_type);
        unsafe {
            gl::BufferSubData(
                buffer_type,
                0,
                (size_of::<T>() * data.len()) as isize,
                data.as_ptr() as _,
            );
        }
        self.bind_buffer(0, buffer_type);
    }

    fn get_uniform_location(&self, program_id: u32, name: &str) -> i32 {
        let terminated_string = format!("{name}\0");
        unsafe { gl::GetUniformLocation(program_id, terminated_string.as_ptr() as _) }
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
                Err(String::from_utf8_unchecked(
                    log.iter().map(|&c| c as u8).collect(),
                ))
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
                Err(String::from_utf8_unchecked(
                    log.iter().map(|&c| c as u8).collect(),
                ))
            } else {
                Ok(program)
            }
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let del_vao: [u32; 1] = [self.quad_vao];
        let del_buf: [u32; 2] = [self.quad_ibo, self.instance_vbo];
        unsafe {
            gl::DeleteVertexArrays(del_vao.len() as i32, del_vao.as_ptr());
            gl::DeleteBuffers(del_buf.len() as i32, del_buf.as_ptr());
            gl::DeleteProgram(self.msdf_program);
            gl::DeleteProgram(self.raster_program);
            gl::DeleteProgram(self.bg_program);
        }
    }
}
