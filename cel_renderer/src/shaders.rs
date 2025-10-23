pub const fg_vert_shader_source: &'static [u8] = b"
    #version 330 core

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

pub const bg_vert_shader_source: &'static [u8] = b"
    #version 330 core

    layout (location = 0) in vec4 inPos;
    layout (location = 1) in vec4 inColor;

    out vec4 bgColor;

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

pub const msdf_frag_shader_source: &'static [u8] = b"
    #version 330 core

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

pub const raster_frag_shader_source: &'static [u8] = b"
    #version 330 core

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

pub const bg_frag_shader_source: &'static [u8] = b"
    #version 330 core

    in vec4 bgColor;

    out vec4 fragColor;

    void main()
    {
        fragColor = bgColor;
    }
\0";

pub const ui_vert_shader_source: &'static [u8] = b"
    #version 330 core

    layout (location = 0) in vec4 inPos;
    layout (location = 1) in vec4 inColor;
    layout (location = 2) in float inRounding;

    out vec4 bgColor;
    out vec2 screenPos;
    flat out vec2 screenCenter;
    flat out vec2 screenHalfSize;
    flat out float rounding;

    uniform float aspect; // width / height

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
        vec2 center = mix(p0, p1, vec2(0.5));
        vec2 halfSize = abs(p1 - p0) * 0.5;

        gl_Position = vec4(pos, 0.0, 1.0)
            * vec4(2.f, -2.f, 1.f, 1.f) // Scale up by 2
            + vec4(-1.f, 1.f, 0.f, 0.f); // Move origin to top left 
        bgColor = inColor;

        // Scale y to match aspect of width 
        screenPos = vec2(pos.x, pos.y / aspect);
        screenCenter = vec2(center.x, center.y / aspect);
        screenHalfSize = vec2(halfSize.x, halfSize.y / aspect);

        rounding = inRounding;
    }
\0";

pub const ui_frag_shader_source: &'static [u8] = b"
    #version 330 core

    in vec4  bgColor;
    in vec2  screenPos;
    flat in vec2 screenCenter; 
    flat in vec2 screenHalfSize; 
    flat in float rounding; // corner radius

    out vec4 fragColor;

    void main() {
        if (rounding > 0.0) {
            vec2 p = screenPos - screenCenter;

            // 2) compute SDF for rounded rect
            vec2 q = abs(p) - (screenHalfSize - vec2(rounding));
            vec2 clamped = max(q, vec2(0.0));
            float dist = length(clamped) - rounding;

            // 3) smoothstep boundary
            float aa = fwidth(dist);  // approximate pixel footprint
            float alpha = 1.0 - smoothstep(0.0, aa, dist);
            fragColor = vec4(bgColor.rgb, bgColor.a * alpha);
        } else {
            fragColor = bgColor;
        }
    }
\0";
