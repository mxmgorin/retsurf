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

// Gap darkness, and the gain that restores the average brightness.
const float DEPTH = 0.5;
const float GAIN = 1.2;
const float TAU = 6.2831853;

void main() {
    vec2 texel = vTexCoord * TextureSize;
    vec4 color = COMPAT_TEXTURE(Texture, (floor(texel) + 0.5) / TextureSize);
    // Whole output rows, so at 2x one is lit and one dark.
    float scale = OutputSize.y / TextureSize.y;
    float phase = floor(fract(texel.y) * scale) / scale;
    float gap = 0.5 - 0.5 * cos(TAU * phase);
    FragColor = vec4(min(color.rgb * (1.0 - DEPTH * gap) * GAIN, vec3(1.0)), 1.0);
}
#endif
