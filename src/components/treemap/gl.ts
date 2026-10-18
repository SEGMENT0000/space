// WebGL2 helpers for the treemap renderer: shader sources, program creation,
// and the instanced attribute layout. One interleaved buffer holds every tile;
// each instance consumes 12 floats. The stride layout below yields those
// floats one attribute each (divisor 1 advances per instance).
//
// State signaling is the instrument-panel pair (INS_BRIEF R10, §1):
//   selection = one fill lift toward the warm accent (the --accent-soft
//   strength, uAccentLift) + a 1px warm-accent inner ring — both read from
//   the CSS --accent token, cached per theme by the renderer's palette;
//   hover = one fill step lighter (uHoverLift) and nothing more — no ring.
// The two lifts ride different channels (accent hue vs white) so a hovered
// selected tile stays unambiguous. aSel is per instance, so the signal
// scales to multi-select, and nothing bleeds outside the tile because both
// the ring and the lifts are drawn strictly inside it. Caches keep their
// fragment-level diagonal hatch on top of any state.
//
// The per-instance alpha (aA) is live: blending is enabled at program
// creation (straight SRC_ALPHA), which is what makes filtered/unavailable
// tiles actually fade toward the canvas ground — without it aA was written
// straight into an opaque framebuffer and the dim was inert.

export const INSTANCE_STRIDE = 48;

/** Hover lift = one fill step lighter (R10). Matches the depth-tint ladder's
 *  single step scale; kept as a uniform so it can be tuned without a recompile. */
export const HOVER_LIFT = 0.12;

export const TILE_VS = `#version 300 es
precision highp float;

layout(location = 0) in float aX;
layout(location = 1) in float aY;
layout(location = 2) in float aW;
layout(location = 3) in float aH;
layout(location = 4) in float aR;
layout(location = 5) in float aG;
layout(location = 6) in float aB;
layout(location = 7) in float aA;
layout(location = 8) in float aSel;
layout(location = 9) in float aHatch;
layout(location = 10) in float aDepthF;
layout(location = 11) in float aHover;

uniform float uTotalScale;
uniform float uTx;
uniform float uTy;
uniform float uDpr;
uniform float uCssW;
uniform float uCssH;

flat out float vAlpha;
flat out float vHover;
flat out float vSelected;
flat out float vHatch;
flat out float vDepth;
flat out vec3 vColor;
flat out vec2 vTopLeft;
flat out vec2 vBottomRight;

void main() {
  vec2 corner = vec2(aX, aY);
  vec2 halfExtent = vec2(aW, aH) * 0.5;
  vec2 topLeft = corner - halfExtent;
  vec2 bottomRight = corner + halfExtent;
  float inset = 0.5 / uTotalScale;
  topLeft += vec2(inset, inset);
  bottomRight -= vec2(inset, inset);
  if (bottomRight.x < topLeft.x) { bottomRight.x = topLeft.x; }
  if (bottomRight.y < topLeft.y) { bottomRight.y = topLeft.y; }
  vec2 cornerPos = vec2(
    (gl_VertexID & 1) == 0 ? topLeft.x : bottomRight.x,
    (gl_VertexID & 2) == 0 ? topLeft.y : bottomRight.y
  );
  // Y edges are given in fragment space (bottom-up); keep the pair ascending so
  // the fragment shader's min() distance math stays positive inside the tile.
  float yA = uCssH - (topLeft.y * uTotalScale + uTy);
  float yB = uCssH - (bottomRight.y * uTotalScale + uTy);
  vTopLeft = vec2(topLeft.x * uTotalScale + uTx, min(yA, yB));
  vBottomRight = vec2(bottomRight.x * uTotalScale + uTx, max(yA, yB));
  vColor = vec3(aR, aG, aB);
  vAlpha = aA;
  vHover = aHover;
  vSelected = aSel;
  vHatch = aHatch;
  // stroke ladder channel (INS_BRIEF §3): depth 0 keeps the full hairline
  // seam; depth-1 children render theirs quieter so the parent boundary
  // stays primary. Flat-qualified like every other instance scalar.
  vDepth = aDepthF;
  // world -> screen (viewport transform) -> clip space
  vec2 screen = cornerPos * uTotalScale + vec2(uTx, uTy);
  vec2 clip = vec2(screen.x / uCssW * 2.0 - 1.0, 1.0 - screen.y / uCssH * 2.0);
  gl_Position = vec4(clip, 0.0, 1.0);
}
`;

export const TILE_FS = `#version 300 es
precision mediump float;

uniform vec3 uStroke;
uniform vec3 uAccent;
uniform float uAccentLift;
uniform float uHoverLift;
uniform float uDpr;

flat in vec3 vColor;
flat in vec2 vTopLeft;
flat in vec2 vBottomRight;
flat in float vAlpha;
flat in float vHover;
flat in float vSelected;
flat in float vHatch;
flat in float vDepth;

out vec4 fragColor;

void main() {
  bool selected = vSelected > 0.5;
  vec3 col = vColor;
  // State = one fill lift on each channel (R10): selection tints toward the
  // warm accent at the --accent-soft alpha; hover steps one fill step
  // lighter toward white. No hover ring — the step plus the full-ink label
  // is the whole hover statement.
  if (selected) {
    col = mix(col, uAccent, uAccentLift);
  } else if (vHover > 0.5) {
    col = mix(col, vec3(1.0), uHoverLift);
  }
  if (vHatch > 0.5) {
    float sx = gl_FragCoord.x / uDpr;
    float sy = gl_FragCoord.y / uDpr;
    if (fract((sx + sy) / 9.0) < 0.5) {
      col = col * 0.88;
    }
  }
  vec2 p = vec2(gl_FragCoord.xy) / uDpr;
  float dx = min(p.x - vTopLeft.x, vBottomRight.x - p.x);
  float dy = min(p.y - vTopLeft.y, vBottomRight.y - p.y);
  float dist = min(dx, dy);
  float px = 1.0 / uDpr;
  // Resting tiles: the half-pixel hairline seam toward the FIELD color —
  // uStroke carries the panel ground behind the canvas (read from CSS by
  // the renderer's palette), so tiles read as cut cells of the panel and
  // the seam behaves identically in both themes. Stepped down by depth
  // (INS_BRIEF §3 stroke ladder): depth 0 keeps the full seam, depth-1
  // child tiles render theirs at 0.6× alpha so a focused parent boundary
  // always reads primary. Same geometry, same single mix.
  if (dist < 1.5 * px * 0.5) {
    col = mix(col, uStroke, vDepth < 0.5 ? 0.25 : 0.15);
  }
  if (selected) {
    // 1px inner ring in the warm accent (the same token the chrome spends
    // on selection), softened by one device pixel so it stays crisp at
    // every DPR. Drawn inside the tile: no bleed.
    float k = 1.0 - smoothstep(1.0 - px, 1.0, dist);
    col = mix(col, uAccent, k);
  }
  fragColor = vec4(col, vAlpha);
}
`;

export interface TileUniforms {
  uTotalScale: WebGLUniformLocation | null;
  uTx: WebGLUniformLocation | null;
  uTy: WebGLUniformLocation | null;
  uDpr: WebGLUniformLocation | null;
  uCssW: WebGLUniformLocation | null;
  uCssH: WebGLUniformLocation | null;
  uStroke: WebGLUniformLocation | null;
  uAccent: WebGLUniformLocation | null;
  uAccentLift: WebGLUniformLocation | null;
  uHoverLift: WebGLUniformLocation | null;
}

export interface TileProgram {
  program: WebGLProgram;
  vao: WebGLVertexArrayObject;
  buffer: WebGLBuffer;
  uniforms: TileUniforms;
  attribLocations: number[];
}

function createShader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) {
    throw new Error("shader");
  }
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader) ?? "unknown error";
    throw new Error(`${type === gl.VERTEX_SHADER ? "vertex" : "fragment"} shader: ${log}`);
  }
  return shader;
}

export function createTileProgram(gl: WebGL2RenderingContext): TileProgram {
  const vs = createShader(gl, gl.VERTEX_SHADER, TILE_VS);
  const fs = createShader(gl, gl.FRAGMENT_SHADER, TILE_FS);
  const program = gl.createProgram();
  if (!program) {
    throw new Error("program");
  }
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error("program link failed");
  }
  // Per-instance alpha is real state, not bookkeeping: straight
  // SRC_ALPHA blending is what lets filtered/hidden tiles fade toward the
  // ground (aA 0.05) instead of painting full color. Without this enable
  // every aA value wrote straight into the opaque framebuffer and the
  // legend filter was visually inert on tiles.
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
  const vao = gl.createVertexArray();
  if (!vao) {
    throw new Error("vertex array");
  }
  gl.useProgram(program);
  gl.bindVertexArray(vao);
  const names = [
    "aX",
    "aY",
    "aW",
    "aH",
    "aR",
    "aG",
    "aB",
    "aA",
    "aSel",
    "aHatch",
    "aDepthF",
    "aHover",
  ];
  const attribLocations: number[] = [];
  // The buffer must exist and be bound before vertexAttribPointer runs: the
  // pointer captures whatever ARRAY_BUFFER is bound at call time.
  const buffer = gl.createBuffer();
  if (!buffer) {
    throw new Error("buffer");
  }
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  for (let i = 0; i < names.length; i++) {
    const loc = gl.getAttribLocation(program, names[i]);
    // -1 means the GLSL compiler dropped the attribute (unreachable code);
    // never happens for aDepthF/aHover — the fragment shader reads both.
    if (loc >= 0) {
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, 1, gl.FLOAT, false, INSTANCE_STRIDE, i * 4);
      gl.vertexAttribDivisor(loc, 1);
    }
    attribLocations.push(loc);
  }
  const uniforms: TileUniforms = {
    uTotalScale: gl.getUniformLocation(program, "uTotalScale"),
    uTx: gl.getUniformLocation(program, "uTx"),
    uTy: gl.getUniformLocation(program, "uTy"),
    uDpr: gl.getUniformLocation(program, "uDpr"),
    uCssW: gl.getUniformLocation(program, "uCssW"),
    uCssH: gl.getUniformLocation(program, "uCssH"),
    uStroke: gl.getUniformLocation(program, "uStroke"),
    uAccent: gl.getUniformLocation(program, "uAccent"),
    uAccentLift: gl.getUniformLocation(program, "uAccentLift"),
    uHoverLift: gl.getUniformLocation(program, "uHoverLift"),
  };
  return { program, vao, buffer, uniforms, attribLocations };
}
