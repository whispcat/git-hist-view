import type { GraphData } from '../worker/protocol';
import type { Palette } from './palette';

/** World-to-screen transform: screen = world * k + (x, y). */
export interface View {
  k: number;
  x: number;
  y: number;
}

/** Node radius in pixels from the view's size metric (0 hides the node). */
export type Radius = (size: number) => number;

const edgeWidth = (count: number) => (count > 0 ? 0.6 + 0.45 * Math.sqrt(Math.min(count, 60)) : 0);
const edgeAlpha = (weight: number) => (weight > 0 ? 0.12 + 0.6 * weight : 0);

const EDGE_VS = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_from;
layout(location = 2) in vec4 a_to;
layout(location = 3) in vec4 a_style; // widthA, widthB, alphaA, alphaB
layout(location = 4) in float a_state; // 0 normal, 1 highlighted, 2 dimmed
uniform float u_t;
uniform vec2 u_view;
uniform vec3 u_camera;
uniform vec3 u_ink;
uniform vec3 u_accent;
out vec4 v_color;
out float v_along;
void main() {
  v_along = a_corner.x;
  vec4 seg = mix(a_from, a_to, u_t);
  vec2 p0 = seg.xy * u_camera.x + u_camera.yz;
  vec2 p1 = seg.zw * u_camera.x + u_camera.yz;
  float width = mix(a_style.x, a_style.y, u_t);
  float alpha = mix(a_style.z, a_style.w, u_t);
  vec2 dir = p1 - p0;
  float len = length(dir);
  vec2 normal = len > 0.0 ? vec2(-dir.y, dir.x) / len : vec2(0.0);
  vec2 p = mix(p0, p1, a_corner.x) + normal * a_corner.y * (width * 0.5 + 0.5);
  gl_Position = vec4(p / u_view * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
  v_color = a_state == 1.0 ? vec4(u_accent, 0.95) : vec4(u_ink, alpha * (a_state == 2.0 ? 0.25 : 1.0));
}`;

const NODE_VS = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_pos;
layout(location = 2) in vec2 a_radius;
layout(location = 3) in float a_slot;
layout(location = 4) in float a_state;
uniform float u_t;
uniform vec2 u_view;
uniform vec3 u_camera;
uniform vec3 u_slots[8];
uniform vec3 u_surface;
out vec2 v_local;
out float v_radius;
out vec3 v_color;
void main() {
  vec2 center = mix(a_pos.xy, a_pos.zw, u_t) * u_camera.x + u_camera.yz;
  v_radius = mix(a_radius.x, a_radius.y, u_t);
  // Two extra pixels hold the surface ring that separates overlapping nodes.
  float extent = v_radius + 2.5;
  v_local = (a_corner * 2.0 - 1.0) * extent;
  gl_Position = vec4((center + v_local) / u_view * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
  vec3 c = u_slots[int(a_slot)];
  v_color = a_state == 2.0 ? mix(c, u_surface, 0.75) : c;
}`;

const NODE_FS = `#version 300 es
precision highp float;
in vec2 v_local;
in float v_radius;
in vec3 v_color;
uniform vec3 u_surface;
out vec4 color;
void main() {
  float d = length(v_local);
  float fill = clamp(v_radius - d + 0.5, 0.0, 1.0);
  float ring = clamp(v_radius + 2.0 - d + 0.5, 0.0, 1.0);
  if (ring <= 0.0 || v_radius <= 0.0) discard;
  color = vec4(mix(u_surface, v_color, fill), ring);
}`;

const EDGE_FS = `#version 300 es
precision mediump float;
in vec4 v_color;
in float v_along;
uniform float u_directed;
out vec4 color;
// Directed edges fade in toward their target, so "a imports b" reads from faint to solid.
void main() { color = vec4(v_color.rgb, v_color.a * mix(1.0 - 0.75 * u_directed, 1.0, v_along)); }`;

function program(gl: WebGL2RenderingContext, vs: string, fs: string) {
  const p = gl.createProgram()!;
  for (const [type, src] of [
    [gl.VERTEX_SHADER, vs],
    [gl.FRAGMENT_SHADER, fs],
  ] as const) {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? 'shader error');
    gl.attachShader(p, s);
  }
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? 'link error');
  return p;
}

/** A VAO with a unit quad plus one interleaved per-instance buffer described by `sizes`. */
function instanced(gl: WebGL2RenderingContext, corners: number[], sizes: number[]) {
  const vao = gl.createVertexArray()!;
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(corners), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  const buffer = gl.createBuffer()!;
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  const stride = sizes.reduce((a, b) => a + b, 0);
  let offset = 0;
  sizes.forEach((size, i) => {
    gl.enableVertexAttribArray(i + 1);
    gl.vertexAttribPointer(i + 1, size, gl.FLOAT, false, stride * 4, offset * 4);
    gl.vertexAttribDivisor(i + 1, 1);
    offset += size;
  });
  return { vao, buffer, stride };
}

interface GraphDraw {
  t: number;
  directed: boolean;
  view: View;
  width: number;
  height: number;
  dpr: number;
}

/** Instanced WebGL2 node-link renderer; edges and nodes tween between keyframes on the GPU. */
export class GraphRenderer {
  private gl: WebGL2RenderingContext;
  private edgeProgram: WebGLProgram;
  private nodeProgram: WebGLProgram;
  private edges;
  private nodes;
  private edgeCount = 0;
  private nodeCount = 0;
  private palette: Palette | null = null;

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext('webgl2', { antialias: true, alpha: false });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    this.edgeProgram = program(gl, EDGE_VS, EDGE_FS);
    this.nodeProgram = program(gl, NODE_VS, NODE_FS);
    this.edges = instanced(gl, [0, -1, 1, -1, 0, 1, 1, 1], [4, 4, 4, 1]);
    this.nodes = instanced(gl, [0, 0, 1, 0, 0, 1, 1, 1], [4, 2, 1, 1]);
  }

  /** `state`: 0 normal, 1 highlighted, 2 dimmed; per node, and per edge via its endpoints. */
  upload(d: GraphData, slot: (group: number) => number, state: (node: number) => number, radius: Radius) {
    const gl = this.gl;
    const n = d.ids.length;
    const nodes = new Float32Array(n * this.nodes.stride);
    for (let i = 0; i < n; i++) {
      nodes.set([...d.pos.subarray(i * 4, i * 4 + 4), radius(d.size[i * 2]), radius(d.size[i * 2 + 1]), slot(d.group[i]), state(i)], i * this.nodes.stride);
    }
    const m = d.ends.length / 2;
    const edges = new Float32Array(m * this.edges.stride);
    for (let e = 0; e < m; e++) {
      const [a, b] = [d.ends[e * 2], d.ends[e * 2 + 1]];
      const [sa, sb] = [state(a), state(b)];
      const edgeState = sa === 1 || sb === 1 ? (sa !== 2 && sb !== 2 ? 1 : 0) : sa === 2 || sb === 2 ? 2 : 0;
      edges.set(
        [
          d.pos[a * 4],
          d.pos[a * 4 + 1],
          d.pos[b * 4],
          d.pos[b * 4 + 1],
          d.pos[a * 4 + 2],
          d.pos[a * 4 + 3],
          d.pos[b * 4 + 2],
          d.pos[b * 4 + 3],
          edgeWidth(d.count[e * 2]),
          edgeWidth(d.count[e * 2 + 1]),
          edgeAlpha(d.weight[e * 2]),
          edgeAlpha(d.weight[e * 2 + 1]),
          edgeState,
        ],
        e * this.edges.stride,
      );
    }
    gl.bindBuffer(gl.ARRAY_BUFFER, this.nodes.buffer);
    gl.bufferData(gl.ARRAY_BUFFER, nodes, gl.DYNAMIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.edges.buffer);
    gl.bufferData(gl.ARRAY_BUFFER, edges, gl.DYNAMIC_DRAW);
    [this.nodeCount, this.edgeCount] = [n, m];
  }

  setPalette(p: Palette) {
    this.palette = p;
  }

  draw(s: GraphDraw) {
    const { gl, palette: p } = this;
    if (!p) return;
    const canvas = gl.canvas as HTMLCanvasElement;
    const [w, h] = [Math.round(s.width * s.dpr), Math.round(s.height * s.dpr)];
    if (canvas.width !== w || canvas.height !== h) [canvas.width, canvas.height] = [w, h];
    gl.viewport(0, 0, w, h);
    const unit = (c: number[]) => c.map((v) => v / 255);
    const surface = unit(p.surface) as [number, number, number];
    gl.clearColor(...surface, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    const common = (prog: WebGLProgram) => {
      gl.useProgram(prog);
      gl.uniform1f(gl.getUniformLocation(prog, 'u_t'), s.t);
      gl.uniform2f(gl.getUniformLocation(prog, 'u_view'), s.width, s.height);
      gl.uniform3f(gl.getUniformLocation(prog, 'u_camera'), s.view.k, s.view.x, s.view.y);
    };
    common(this.edgeProgram);
    gl.uniform3fv(gl.getUniformLocation(this.edgeProgram, 'u_ink'), unit(p.ink2));
    gl.uniform3fv(gl.getUniformLocation(this.edgeProgram, 'u_accent'), unit(p.accent));
    gl.uniform1f(gl.getUniformLocation(this.edgeProgram, 'u_directed'), s.directed ? 1 : 0);
    gl.bindVertexArray(this.edges.vao);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.edgeCount);

    common(this.nodeProgram);
    gl.uniform3fv(gl.getUniformLocation(this.nodeProgram, 'u_slots'), p.categorical.flatMap(unit));
    gl.uniform3fv(gl.getUniformLocation(this.nodeProgram, 'u_surface'), surface);
    gl.bindVertexArray(this.nodes.vao);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.nodeCount);
  }

  dispose() {
    this.gl.getExtension('WEBGL_lose_context')?.loseContext();
  }
}
