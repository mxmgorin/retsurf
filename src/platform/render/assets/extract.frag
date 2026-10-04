// The game back at its own resolution: one sample per game pixel, at its centre.
uniform sampler2D u_page;
uniform vec2 u_size;
uniform vec2 u_source_size;
uniform vec4 u_source_rect;

void main() {
    vec2 px = u_source_rect.xy + gl_FragCoord.xy * u_source_rect.zw / u_source_size;
    f_color = texture2D(u_page, px / u_size);
}
