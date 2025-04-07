use cel_core::ansi::{CellContent, CursorStyle, StyleFlags, TerminalState};
use std::time::{Duration, SystemTime};
use std::{
    cell::RefCell,
    mem::size_of,
    ptr::{null, null_mut},
    rc::Rc,
};

use crate::font::GlyphMetrics;
use crate::{
    font::{Font, RenderType},
    glchk,
    util::Util,
};

const MAX_CHARACTERS: u32 = 50000;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct FgQuadData {
    pub position: [f32; 4],  // x0, y0, x1, y1
    pub tex_coord: [f32; 4], // u0, v0, u1, v1
    pub color: [f32; 3],     // r, g, b (fg)
    pub flags: StyleFlags
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct BgQuadData {
    pub position: [f32; 4],  // x0, y0, x1, y1
    pub color: [f32; 3],     // r, g, b (bg)
    pub padding: u32
}

#[derive(Copy, Clone, Default)]
pub struct RenderStats {
    pub num_fg_instances: u32,
    pub num_bg_instances: u32,
    pub wrapped_line_count: u32,
    pub rendered_line_count: u32
}

pub struct RenderConstants {
    pub char_size_x_px: f32,
    pub char_size_y_px: f32,
    pub char_size_x_screen: f32,
    pub char_size_y_screen: f32,
    pub atlas_pixel_size: f32
}

pub struct Renderer {
    msdf_program: u32,
    raster_program: u32,
    bg_program: u32,
    fg_quad_vao: u32,
    bg_quad_vao: u32,
    quad_ibo: u32,
    instance_vbo: u32,
    width: u32,
    height: u32,
    scale: [f32; 2],
    font: Rc<RefCell<Font>>,
}

impl Renderer {
    pub fn new(width: i32, height: i32, scale: [f32; 2], default_font: Rc<RefCell<Font>>) -> Self {
        // Generate buffers
        let mut fg_quad_vao: u32 = 0;
        let mut bg_quad_vao: u32 = 0;
        let mut instance_vbo: u32 = 0;
        let mut quad_ibo: u32 = 0;
        unsafe {
            // FG VAO
            gl::GenVertexArrays(1, &mut fg_quad_vao);
            gl::BindVertexArray(fg_quad_vao);

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

            // FG VAO attributes
            let instance_stride = size_of::<FgQuadData>() as i32;
            let pos_stride = size_of::<f32>() as i32 * 4;
            let coord_stride = size_of::<f32>() as i32 * 4 + pos_stride;
            let color_stride = size_of::<f32>() as i32 * 3 + coord_stride;
            gl::VertexAttribPointer(0, 4, gl::FLOAT, gl::FALSE, instance_stride, null());
            gl::VertexAttribPointer(1, 4, gl::FLOAT, gl::FALSE, instance_stride, pos_stride as _);
            gl::VertexAttribPointer(2, 3, gl::FLOAT, gl::FALSE, instance_stride, coord_stride as _);
            gl::VertexAttribPointer(3, 1, gl::UNSIGNED_INT, gl::FALSE, instance_stride, color_stride as _);
            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);
            gl::EnableVertexAttribArray(2);
            gl::EnableVertexAttribArray(3);
            gl::VertexAttribDivisor(0, 1);
            gl::VertexAttribDivisor(1, 1);
            gl::VertexAttribDivisor(2, 1);
            gl::VertexAttribDivisor(3, 1);

            // BG VAO
            gl::GenVertexArrays(1, &mut bg_quad_vao);
            gl::BindVertexArray(bg_quad_vao);

            // Bind IBO
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, quad_ibo);

            // BG VAO attributes
            let instance_stride = size_of::<BgQuadData>() as i32;
            let pos_stride = size_of::<f32>() as i32 * 4;
            gl::VertexAttribPointer(0, 4, gl::FLOAT, gl::FALSE, instance_stride, null());
            gl::VertexAttribPointer(1, 3, gl::FLOAT, gl::FALSE, instance_stride, pos_stride as _);
            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribDivisor(0, 1);
            gl::VertexAttribDivisor(1, 1);
        }

        let fg_vert_shader_source = b"
            #version 400 core

            layout (location = 0) in vec4 inPos;
            layout (location = 1) in vec4 inTexCoord;
            layout (location = 2) in vec3 inColor;
            layout (location = 3) in uint inFlags;

            out vec2 texCoord;
            out vec3 fgColor;
            flat out uint flags;

            uniform mat4 model;

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

                gl_Position = model * vec4(pos, 0.0, 1.0)
                    * vec4(2.f, -2.f, 1.f, 1.f) // Scale up by 2 & flip y
                    + vec4(-1.f, 1.f, 0.f, 0.f); // Move origin to top left 
                texCoord = coord;
                fgColor = inColor;
                flags = inFlags;
            }
        \0";

        let bg_vert_shader_source = b"
            #version 400 core

            layout (location = 0) in vec4 inPos;
            layout (location = 1) in vec3 inColor;

            out vec3 bgColor;

            uniform mat4 model;

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

                gl_Position = model * vec4(pos, 0.0, 1.0)
                    * vec4(2.f, -2.f, 1.f, 1.f) // Scale up by 2
                    + vec4(-1.f, 1.f, 0.f, 0.f); // Move origin to top left 
                bgColor = inColor;
            }
        \0";

        let msdf_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;
            in vec3 fgColor;
            flat in uint flags;

            out vec4 fragColor;

            uniform sampler2D atlasTex;
            uniform float pixelRange;

            float median(float r, float g, float b, float a) {
                return max(min(r, g), min(max(r, g), b));
            }

            void main()
            {
                float sdFactor = 1.05 + (flags & 1U) * 0.3 - (flags & 2U) * 0.05;
                vec4 msd = texture(atlasTex, texCoord);
                float sd = median(msd.r, msd.g, msd.b, msd.a) * sdFactor;
                float screenPxDistance = pixelRange * (sd - 0.5);
                float opacity = clamp(screenPxDistance + 0.5, 0.0, 1.0);
                
                fragColor = vec4(fgColor, opacity);
            }
        \0";

        let raster_frag_shader_source = b"
            #version 400 core

            in vec2 texCoord;
            in vec3 fgColor;

            out vec4 fragColor;

            uniform sampler2D atlasTex;

            void main()
            {
                vec4 color = texture(atlasTex, texCoord);
                fragColor = vec4(color.rgb * fgColor, color.a);
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
        let fg_vert_shader = match Self::compile_shader(gl::VERTEX_SHADER, fg_vert_shader_source) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to compile vertex shader: {}", msg),
        };
        let bg_vert_shader = match Self::compile_shader(gl::VERTEX_SHADER, bg_vert_shader_source) {
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
        let msdf_program = match Self::link_program(fg_vert_shader, msdf_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link msdf program: {}", msg),
        };
        let raster_program = match Self::link_program(fg_vert_shader, raster_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link raster program: {}", msg),
        };
        let bg_program = match Self::link_program(bg_vert_shader, bg_frag_shader) {
            Ok(id) => id,
            Err(msg) => panic!("Failed to link bg program: {}", msg),
        };

        // Free shaders
        unsafe {
            gl::DeleteShader(fg_vert_shader);
            gl::DeleteShader(bg_vert_shader);
            gl::DeleteShader(msdf_frag_shader);
            gl::DeleteShader(raster_frag_shader);
            gl::DeleteShader(bg_frag_shader);
        }

        let mut obj = Self {
            msdf_program,
            raster_program,
            bg_program,
            fg_quad_vao,
            bg_quad_vao,
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

    pub fn enable_scissor(&self) {
        unsafe { gl::Enable(gl::SCISSOR_TEST); }
    }

    pub fn disable_scissor(&self) {
        unsafe { gl::Disable(gl::SCISSOR_TEST); }
    }

    pub fn update_scissor_screen(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let scaled_width = (width * self.scale[0] * self.width as f32) as i32;
        let scaled_height = (height * self.scale[1] * self.height as f32) as i32;
        let scaled_x = (x * self.scale[0] * self.width as f32) as i32;
        // Flip y
        let scaled_y = (((1.0 - height - y) * self.height as f32) * self.scale[1]) as i32;
        unsafe { gl::Scissor(scaled_x, scaled_y, scaled_width, scaled_height) }
    }

    pub fn compute_max_lines(&self, rc: &RenderConstants, screen_height: f32) -> u32 {
        let lines_per_screen = (1.0 / rc.char_size_y_screen).floor();

        (lines_per_screen * screen_height) as u32
    }

    /// Returns rendered line count
    pub fn render_terminal(
        &mut self,
        terminal_state: &TerminalState,
        screen_size: &[f32; 2],
        screen_offset: &[f32; 2],
        chars_per_line: u32,
        lines_per_screen: u32,
        line_offset: u32,
        wrap: bool,
        debug_line_number: bool,
        debug_col_number: bool,
        debug_show_cursor: bool,
    ) -> RenderStats {
        let mut stats = RenderStats::default();

        // Setup render state
        let rc = self.compute_render_constants(screen_size[0], chars_per_line);

        let timestamp_seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::new(0, 0))
            .as_secs_f64();
        // Clamp base position to nearest pixel
        let base_x = ((screen_offset[0] / rc.char_size_x_screen) * rc.char_size_x_px).floor() / rc.char_size_x_px;
        let base_y = ((screen_offset[1] / rc.char_size_y_screen) * rc.char_size_y_px).floor() / rc.char_size_y_px;
        let mut x = base_x;
        let mut y = base_y - 1.0;
        let mut should_render_cursor = debug_show_cursor
            || (terminal_state.cursor_state.visible
                && (!terminal_state.cursor_state.blinking || timestamp_seconds.fract() <= 0.5));
        let mut rendered_line_count = 0;
        let mut max_line_count = 0;
        let mut msdf_quads: Vec<FgQuadData> = vec![]; // TODO: reuse
        let mut raster_quads: Vec<FgQuadData> = vec![];
        let mut bg_quads: Vec<BgQuadData> = vec![];

        //
        // Populate vertex buffers
        //

        let line_offset = line_offset as usize;
        'outer: for line_idx in line_offset..(line_offset + lines_per_screen as usize) {
            rendered_line_count += 1;
            x = base_x;
            y += 1.0;

            let line_exists = line_idx < terminal_state.screen_buffer.len();
            let y_pos_screen = y * rc.char_size_y_screen;

            // Handle offscreen lines
            if y_pos_screen < 0.0 || y_pos_screen > 1.0 {
                if !line_exists {
                    break;
                }

                let line = &terminal_state.screen_buffer[line_idx];
                let line_occupancy = (line.len().max(1) - 1) as u32 / chars_per_line + 1;
                if y_pos_screen + rc.char_size_y_screen * line_occupancy as f32 <= 0.0 {
                    // Skip rendering iff this line is completely offscreen
                    rendered_line_count += line_occupancy - 1;
                    y += (line_occupancy  - 1) as f32;
                    max_line_count = rendered_line_count;

                    continue;
                }
            }

            // Render cursor
            if should_render_cursor {
                let cursor = &terminal_state.global_cursor;
                if cursor[1] == line_idx {
                    // Compute absolute position to account for wraps
                    should_render_cursor = false;
                    let width = match terminal_state.cursor_state.style {
                        // Adjust width of cursor based on char width
                        CursorStyle::Bar => 0.15,
                        _ => {
                            if cursor[1] < terminal_state.screen_buffer.len() &&
                               cursor[0] < terminal_state.screen_buffer[cursor[1]].len() {
                                match terminal_state.screen_buffer[cursor[1]][cursor[0]].elem {
                                    CellContent::Char(_, w) => w as f32,
                                    CellContent::Grapheme(_, w) => w as f32,
                                    _ => 1.0
                                }
                            } else {
                                1.0
                            }
                        }
                    };
                    let height = match terminal_state.cursor_state.style {
                        CursorStyle::Underline => 0.09,
                        _ => 1.0,
                    };
                    let pos_min = [
                        base_x + (cursor[0] % chars_per_line as usize) as f32,
                        y + (cursor[0] / chars_per_line as usize) as f32,
                    ];
                    Self::push_fg_quad(
                        &[1.0, 0.0, 0.0],
                        &[rc.atlas_pixel_size, rc.atlas_pixel_size],
                        &[rc.atlas_pixel_size, rc.atlas_pixel_size],
                        &[
                            pos_min[0],
                            pos_min[1] + 1.0 - height
                        ],
                        &[
                            pos_min[0] + width,
                            pos_min[1] + 1.0
                        ],
                        StyleFlags::default(),
                        &mut raster_quads,
                    );
                }
            }

            if !line_exists {
                // Continue instead of breaking here so we can render the cursor
                continue;
            }

            max_line_count = rendered_line_count;

            // Store bg color per line for optimization
            let mut prev_bg_color = terminal_state.background_color;

            let line = &terminal_state.screen_buffer[line_idx];
            for char_idx in 0..line.len() {
                if rendered_line_count > lines_per_screen {
                    max_line_count = lines_per_screen;
                    break 'outer;
                }

                let max_x = base_x + chars_per_line as f32 - 0.001;
                let should_wrap = wrap && x >= max_x;
                if should_wrap {
                    max_line_count += 1;
                    rendered_line_count += 1;
                    stats.wrapped_line_count += 1;
                    x = base_x;
                    y += 1.0;
                    prev_bg_color = terminal_state.background_color;
                }

                let elem = &line[char_idx];

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

                    Self::push_bg_quad(
                        bg_color,
                        &[x, y],
                        &[x + 1.0, y + 1.0],
                        &mut bg_quads
                    );

                    stats.num_bg_instances += 1;
                } else if elem.style.bg_color.is_some() {
                    Self::extend_previous_quad(x + 1.0, &mut bg_quads);
                }

                let mut char_to_draw = None;
                let mut width = 1.0;
                let mut skip = false;
                match &elem.elem {
                    CellContent::Char(c, c_width) => {
                        // Skip rendering if this is a whitespace char
                        if c.is_whitespace() || *c == '\0' {
                            skip = true;
                        } else {
                            char_to_draw = Some(*c);
                            width = *c_width as f32;
                        }
                    },
                    CellContent::Grapheme(str, width) => {
                        stats.num_fg_instances += self.push_unicode_quad(
                            str,
                            &rc,
                            fg_color,
                            &[x, y],
                            *width as f32,
                            elem.style.flags,
                            &mut msdf_quads,
                            &mut raster_quads,
                        );
                    },
                    CellContent::Continuation(_) => skip = true,
                    CellContent::Empty => skip = true
                };

                if skip {
                    x += 1.0;
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
                    stats.num_fg_instances += 1;
                    self.push_char_quad(
                        char_to_draw,
                        &rc,
                        fg_color,
                        &[x, y],
                        width,
                        elem.style.flags,
                        &mut msdf_quads,
                        &mut raster_quads,
                    );
                }

                x += 1.0;
            }
        }

        self.draw_text_quads(&rc, &[0.0, 0.0], &bg_quads, &msdf_quads, &raster_quads);

        stats.rendered_line_count = max_line_count;
        stats
    }

    pub fn draw_quad(
        &self,
        screen_offset: &[f32; 2],
        bg_size_screen: &[f32; 2],
        bg_color: &[f32; 3],
    ) {
        let mut bg_quads: Vec<BgQuadData> = vec![];

        // Draw background, separately from text
        let bg_model_mat: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            screen_offset[0], screen_offset[1], 0.0, 1.0,
        ];
        Self::push_bg_quad(
            bg_color,
            &[0.0, 0.0],
            &bg_size_screen,
            &mut bg_quads
        );
        self.draw_background_quads(&bg_quads, &bg_model_mat);
    }

    // Origin top left to bottom right
    pub fn draw_text(
        &mut self,
        char_height_px: f32,
        screen_offset: &[f32; 2],
        bg_size_screen: &[f32; 2],
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        centered: bool,
        text: &str,
    ) {
        let rc = self.compute_render_constants_from_height(char_height_px);

        let mut x = 0.0;
        let mut y = 0.0;
        let mut msdf_quads: Vec<FgQuadData> = vec![]; // TODO: reuse
        let mut raster_quads: Vec<FgQuadData> = vec![];

        for c in text.chars() {
            if c == '\n' {
                x = 0.0;
                y += 1.0;
                continue;
            }

            if c.is_whitespace() || c == '\0' {
                x += 1.0;
                continue;
            }

            self.push_char_quad(
                c,
                &rc,
                fg_color,
                &[x, y],
                1.0,
                StyleFlags::default(),
                &mut msdf_quads,
                &mut raster_quads,
            );

            x += 1.0;
        }

        self.draw_quad(screen_offset, bg_size_screen, bg_color);

        // Draw text, centered on background
        let centered_offset = [
            screen_offset[0] + (bg_size_screen[0] - x * rc.char_size_x_screen) * 0.5,
            screen_offset[1] + (bg_size_screen[1] - rc.char_size_y_screen) * 0.5,
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
        width_screen: f32,
        chars_per_line: u32
    ) -> RenderConstants {
        let width_px = width_screen * self.width as f32;
        let face_metrics = *self.font.as_ref().borrow().get_primary_metrics();
        let char_size_x_px = (width_px / chars_per_line as f32).round();
        let char_size_y_px = (char_size_x_px * face_metrics.height).round();
        let char_size_x_screen = char_size_x_px / self.width as f32;
        let char_size_y_screen = char_size_y_px / self.height as f32;
        let atlas_size = self.font.as_ref().borrow().get_atlas_size();
        let atlas_pixel_size = 1.0 / atlas_size as f32;

        RenderConstants {
            char_size_x_px,
            char_size_y_px,
            char_size_x_screen,
            char_size_y_screen,
            atlas_pixel_size
        }
    }

    pub fn compute_render_constants_from_height(
        &self,
        height_px: f32,
    ) -> RenderConstants {
        let face_metrics = *self.font.as_ref().borrow().get_primary_metrics();
        let char_size_y_px = height_px;
        let char_size_x_px = (char_size_y_px / face_metrics.height).round();
        let char_size_x_screen = char_size_x_px / self.width as f32;
        let char_size_y_screen = char_size_y_px / self.height as f32;
        let atlas_size = self.font.as_ref().borrow().get_atlas_size();
        let atlas_pixel_size = 1.0 / atlas_size as f32;

        RenderConstants {
            char_size_x_px,
            char_size_y_px,
            char_size_x_screen,
            char_size_y_screen,
            atlas_pixel_size
        }
    }

    pub fn to_screen_i32(&self, pos: [i32; 2]) -> [f32; 2] {
        [pos[0] as f32 / self.width as f32, pos[1] as f32 / self.height as f32]
    }

    pub fn to_screen_u32(&self, pos: [u32; 2]) -> [f32; 2] {
        [pos[0] as f32 / self.width as f32, pos[1] as f32 / self.height as f32]
    }

    pub fn to_screen_f32(&self, pos: [f32; 2]) -> [f32; 2] {
        [pos[0] / self.width as f32, pos[1] / self.height as f32]
    }

    pub fn get_width(&self) -> u32 { self.width }
    pub fn get_height(&self) -> u32 { self.height }
    pub fn get_aspect_ratio(&self) -> f32 { self.width as f32 / self.height as f32 }

    fn extend_previous_quad(new_x: f32, quads: &mut Vec<BgQuadData>) {
        match quads.last_mut() {
            Some(quad) => quad.position[2] = new_x,
            None => {}
        }
    }

    // Min: TL, max: BR
    fn push_fg_quad(
        fg_color: &[f32; 3],
        uv_min: &[f32; 2],
        uv_max: &[f32; 2],
        pos_min: &[f32; 2],
        pos_max: &[f32; 2],
        flags: StyleFlags,
        arr: &mut Vec<FgQuadData>,
    ) {
        arr.push(FgQuadData {
            position: [pos_min[0], pos_max[1], pos_max[0], pos_min[1]],
            tex_coord: [uv_min[0], uv_max[1], uv_max[0], uv_min[1]],
            color: *fg_color,
            flags
        });
    }

    // Min: TL, max: BR
    fn push_bg_quad(
        bg_color: &[f32; 3],
        pos_min: &[f32; 2],
        pos_max: &[f32; 2],
        arr: &mut Vec<BgQuadData>
    ) {
        arr.push(BgQuadData {
            position: [pos_min[0], pos_max[1], pos_max[0], pos_min[1]],
            color: *bg_color,
            padding: 0
        });
    }

    fn push_glyph_quad(
        metrics: &GlyphMetrics,
        rc: &RenderConstants,
        fg_color: &[f32; 3],
        pos: &[f32; 2], // In character space
        width: f32, // In character space
        flags: StyleFlags,
        msdf_arr: &mut Vec<FgQuadData>,
        raster_arr: &mut Vec<FgQuadData>
    ) {
        let glyph_bound = &metrics.glyph_bound;
        let atlas_uv = &metrics.atlas_uv;
        let top = pos[1] + 1.0 - glyph_bound.top;
        let bottom = pos[1] + 1.0 - glyph_bound.bottom;
        match metrics.render_type {
            RenderType::MSDF => {
                Self::push_fg_quad(
                    fg_color,
                    &[atlas_uv.left, atlas_uv.top],
                    &[atlas_uv.right, atlas_uv.bottom],
                    &[pos[0] + glyph_bound.left, top],
                    &[pos[0] + glyph_bound.right, bottom],
                    flags,
                    msdf_arr,
                )
            },
            RenderType::RASTER => {
                // Center position based on cell width and maintain aspect ratio
                let real_width = glyph_bound.height() * (rc.char_size_y_px / rc.char_size_x_px);
                let left = pos[0] + (width * 0.5) - (real_width * 0.5);
                Self::push_fg_quad(
                    // Ignore fg color
                    &[1.0, 1.0, 1.0],
                    &[atlas_uv.left, atlas_uv.top],
                    &[atlas_uv.right, atlas_uv.bottom],
                    &[left, top],
                    &[left + real_width, bottom],
                    flags,
                    raster_arr,
                )
            }
        }
    }

    fn push_char_quad(
        &mut self,
        c: char,
        rc: &RenderConstants,
        fg_color: &[f32; 3],
        pos: &[f32; 2], // In character space
        width: f32, // In character space
        flags: StyleFlags,
        msdf_arr: &mut Vec<FgQuadData>,
        raster_arr: &mut Vec<FgQuadData>,
    ) {
        let mut mut_font = self.font.as_ref().borrow_mut();
        let glyph_metrics = &mut_font.get_glyph_data(c);
        Self::push_glyph_quad(
            &glyph_metrics,
            rc,
            fg_color,
            pos,
            width,
            flags,
            msdf_arr,
            raster_arr
        );
    }

    fn push_unicode_quad(
        &mut self,
        str: &str,
        rc: &RenderConstants,
        fg_color: &[f32; 3],
        pos: &[f32; 2], // In character space
        width: f32, // In character space
        flags: StyleFlags,
        msdf_arr: &mut Vec<FgQuadData>,
        raster_arr: &mut Vec<FgQuadData>,
    ) -> u32 {
        let mut count = 0;
        let mut mut_font = self.font.as_ref().borrow_mut();
        for metrics in mut_font.get_grapheme_data(str).iter() {
            count += 1;
            Self::push_glyph_quad(
                metrics,
                rc,
                fg_color,
                pos,
                width,
                flags,
                msdf_arr,
                raster_arr
            );
        }

        count
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
        bg_quads: &Vec<BgQuadData>,
        msdf_quads: &Vec<FgQuadData>,
        raster_quads: &Vec<FgQuadData>,
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

    fn draw_msdf_quads(&self, arr: &[FgQuadData], model: &[f32; 16], pixel_range: f32) {
        let font = self.font.as_ref().borrow();

        // Bind program data
        unsafe {
            gl::UseProgram(self.msdf_program);
            gl::BindVertexArray(self.fg_quad_vao);
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

        self.enable_blending();
        self.draw_indexed_instanced(arr.len() as i32);
        self.disable_blending();
    }

    fn draw_raster_quads(&self, arr: &[FgQuadData], model: &[f32; 16]) {
        let font = self.font.as_ref().borrow();

        // Bind program data
        unsafe {
            gl::UseProgram(self.raster_program);
            gl::BindVertexArray(self.fg_quad_vao);
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

        self.enable_blending();
        self.draw_indexed_instanced(arr.len() as i32);
        self.disable_blending();
    }

    fn draw_background_quads(&self, arr: &[BgQuadData], model: &[f32; 16]) {
        // Bind program data
        unsafe {
            gl::UseProgram(self.bg_program);
            gl::BindVertexArray(self.bg_quad_vao);
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
    }

    fn enable_backface_culling(&self) {
        unsafe {
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
        }
    }

    fn enable_blending(&self) {
        unsafe {
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        }
    }

    fn disable_blending(&self) {
        unsafe {
            gl::Disable(gl::BLEND);
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
            gl::BufferData(
                buffer_type,
                (size_of::<T>() * data.len()) as isize,
                data.as_ptr() as _,
                gl::STATIC_DRAW,
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
        let del_vao: [u32; 2] = [self.fg_quad_vao, self.bg_quad_vao];
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
