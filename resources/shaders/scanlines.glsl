// Scanlines: a dark gap between the game's rows.

#if defined(VERTEX)

#if __VERSION__ >= 130
#define COMPAT_VARYING out
#define COMPAT_ATTRIBUTE in
#else
#define COMPAT_VARYING varying
#define COMPAT_ATTRIBUTE attribute
#endif

COMPAT_ATTRIBUTE vec4 VertexCoord;
COMPAT_ATTRIBUTE vec4 TexCoord;
COMPAT_VARYING vec2 vTexCoord;
uniform mat4 MVPMatrix;

void main() {
    gl_Position = MVPMatrix * VertexCoord;
    vTexCoord = TexCoord.xy;
}

#elif defined(FRAGMENT)

#ifdef GL_ES
precision mediump float;
#endif

#if __VERSION__ >= 130
#define COMPAT_VARYING in
#define COMPAT_TEXTURE texture
out vec4 FragColor;
#else
#define COMPAT_VARYING varying
#define COMPAT_TEXTURE texture2D
#define FragColor gl_FragColor
#endif

uniform sampler2D Texture;
uniform vec2 TextureSize;
uniform vec2 OutputSize;
COMPAT_VARYING vec2 vTexCoord;

// How much the gap row darkens.
const float DEPTH = 0.5;

void main() {
    vec2 texel = vTexCoord * TextureSize;
    vec4 color = COMPAT_TEXTURE(Texture, (floor(texel) + 0.5) / TextureSize);
    // The gap is the last output row of each game row, so a scale that is not
    // a whole number still gives every row exactly one.
    float scale = OutputSize.y / TextureSize.y;
    float gap = step(1.0, floor(texel.y + 1.0 / scale) - floor(texel.y));
    // One row in `scale` is dark; this keeps the average brightness.
    float gain = 1.0 / (1.0 - DEPTH / max(scale, 1.0));
    FragColor = vec4(min(color.rgb * (1.0 - DEPTH * gap) * gain, vec3(1.0)), 1.0);
}
#endif
