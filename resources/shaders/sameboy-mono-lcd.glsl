// SameBoy's monochrome LCD shader: a pixel grid with a drop shadow, ported to RetroArch's single-pass GLSL from
// https://github.com/LIJI32/SameBoy/blob/c458e7c5d2d350fb37a1931c40da9f758d28d240/Shaders/MonoLCD.fsh
//
// Copyright (c) 2015-2026 Lior Halphon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

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
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
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

// SameBoy's MasterShader.fsh, cut to what scale() needs: it works in linear
// light, and its images run top-down where this one runs bottom-up.
#define STATIC
#define GAMMA 2.2

vec4 sb_texture(sampler2D t, vec2 pos)
{
    return pow(COMPAT_TEXTURE(t, vec2(pos.x, 1.0 - pos.y)), vec4(GAMMA));
}

vec4 texture_relative(sampler2D t, vec2 pos, vec2 offset)
{
    return sb_texture(t, (floor(pos * TextureSize) + offset + vec2(0.5, 0.5)) / TextureSize);
}

#define texture sb_texture

// SameBoy's Shaders/MonoLCD.fsh, unmodified:
#define SCANLINE_DEPTH 0.25
#define BLOOM 0.4

STATIC vec4 scale(sampler2D image, vec2 position, vec2 input_resolution, vec2 output_resolution)
{
    vec2 pixel = position * input_resolution - vec2(0.5, 0.5);

    vec4 q11 = texture(image, (floor(pixel) + 0.5) / input_resolution);
    vec4 q12 = texture(image, (vec2(floor(pixel.x), ceil(pixel.y)) + 0.5) / input_resolution);
    vec4 q21 = texture(image, (vec2(ceil(pixel.x), floor(pixel.y)) + 0.5) / input_resolution);
    vec4 q22 = texture(image, (ceil(pixel) + 0.5) / input_resolution);

    vec2 s = smoothstep(0., 1., fract(pixel));

    vec4 r1 = mix(q11, q21, s.x);
    vec4 r2 = mix(q12, q22, s.x);
    
    vec2 pos = fract(position * input_resolution);
    vec2 sub_pos = pos * 6.0;

    float multiplier = 1.0;
    
    if (sub_pos.y < 1.0) {
        multiplier *= sub_pos.y * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
    }
    else if (sub_pos.y > 5.0) {
        multiplier *= (6.0 - sub_pos.y) * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
    }
    
    if (sub_pos.x < 1.0) {
        multiplier *= sub_pos.x * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
    }
    else if (sub_pos.x > 5.0) {
        multiplier *= (6.0 - sub_pos.x) * SCANLINE_DEPTH + (1.0 - SCANLINE_DEPTH);
    }

    vec4 pre_shadow = mix(texture(image, position) * multiplier, mix(r1, r2, s.y), BLOOM);
    pre_shadow.a = 1.0;
    pixel += vec2(-0.6, -0.8);
    
    q11 = texture(image, (floor(pixel) + 0.5) / input_resolution);
    q12 = texture(image, (vec2(floor(pixel.x), ceil(pixel.y)) + 0.5) / input_resolution);
    q21 = texture(image, (vec2(ceil(pixel.x), floor(pixel.y)) + 0.5) / input_resolution);
    q22 = texture(image, (ceil(pixel) + 0.5) / input_resolution);
   
    r1 = mix(q11, q21, fract(pixel.x));
    r2 = mix(q12, q22, fract(pixel.x));
    
    vec4 shadow = mix(r1, r2, fract(pixel.y));
    return mix(min(shadow, pre_shadow), pre_shadow, 0.75);
}

void main()
{
    vec2 position = vec2(vTexCoord.x, 1.0 - vTexCoord.y);
    FragColor = pow(scale(Texture, position, TextureSize, OutputSize), vec4(1.0 / GAMMA));
}

#endif
