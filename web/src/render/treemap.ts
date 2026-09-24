import type { ColorMode } from '../state/url';
import type { TreemapData } from '../worker/protocol';
import { CHURN_MAX, type Palette, RAMP_SIZE } from './palette';

export interface Camera {
  k: number;
  x: number;
  y: number;
}

const FLOATS = 14;

const VERTEX = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_from;
layout(location = 2) in vec4 a_to;
layout(location = 3) in vec2 a_churn;
layout(location = 4) in vec2 a_author;
layout(location = 5) in float a_lang;
layout(location = 6) in float a_flags;
uniform float u_t;
uniform vec2 u_view;
uniform vec3 u_camera;
uniform int u_mode;
uniform float u_spot;
uniform float u_log_max;
uniform vec3 u_slots[8];
uniform vec3 u_zero;
uniform vec3 u_dir;
uniform vec3 u_ink;
uniform sampler2D u_ramp;
out vec3 v_color;

void main() {
  vec4 g = mix(a_from, a_to, u_t);
  int flags = int(a_flags);
  bool dir = (flags & 1) == 1;
  // Files are inset by one screen pixel on each side, leaving a 2px surface gap between neighbours.
  float inset = dir ? 0.0 : 1.0 / u_camera.x;
  vec2 size = max(g.zw - 2.0 * inset, vec2(0.0));
  vec2 p = (g.xy + inset + a_corner * size) * u_camera.x + u_camera.yz;
  gl_Position = vec4(p / u_view * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);

  if (dir) {
    v_color = mix(u_dir, u_ink, min(0.035 * float(flags >> 2), 0.14));
  } else if (u_mode == 0) {
    float lines = mix(a_churn.x, a_churn.y, u_t);
    v_color = lines < 0.5 ? u_zero : texture(u_ramp, vec2(min(log(1.0 + lines) / u_log_max, 1.0), 0.5)).rgb;
  } else {
    float slot = u_mode == 1 ? (u_t < 0.5 ? a_author.x : a_author.y) : a_lang;
    v_color = u_slots[int(slot)];
    if (u_spot >= 0.0 && slot != u_spot) v_color = mix(v_color, u_zero, 0.8);
  }
}`;

const FRAGMENT = `#version 300 es
precision mediump float;
in vec3 v_color;
out vec4 color;
void main() { color = vec4(v_color, 1.0); }`;

const MODES: Record<ColorMode, number> = { churn: 0, author: 1, language: 2 };

function compile(gl: WebGL2RenderingContext, type: number, source: string) {
  const shader = gl.createShader(type)!;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? 'shader error');
  return shader;
}

interface DrawState {
  t: number;
  camera: Camera;
  mode: ColorMode;
  spot: number;
  width: number;
  height: number;
  dpr: number;
}

/** Instanced WebGL2 treemap: one quad per cell, tweened on the GPU so a frame is a handful of uniforms. */
export class TreemapRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private vao: WebGLVertexArrayObject;
  private instances: WebGLBuffer;
  private ramp: WebGLTexture;
  private uniforms: Record<string, WebGLUniformLocation | null> = {};
  private count = 0;
  private palette: Palette | null = null;

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext('webgl2', { antialias: false, alpha: false, premultipliedAlpha: false });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    this.program = gl.createProgram()!;
    gl.attachShader(this.program, compile(gl, gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(this.program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(this.program);
    if (!gl.getProgramParameter(this.program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(this.program) ?? 'link error');
    for (const name of ['u_t', 'u_view', 'u_camera', 'u_mode', 'u_spot', 'u_log_max', 'u_slots', 'u_zero', 'u_dir', 'u_ink', 'u_ramp']) {
      this.uniforms[name] = gl.getUniformLocation(this.program, name);
    }

    this.vao = gl.createVertexArray()!;
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

    this.instances = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    const layout: [number, number][] = [
      [4, 0],
      [4, 4],
      [2, 8],
      [2, 10],
      [1, 12],
      [1, 13],
    ];
    layout.forEach(([size, offset], i) => {
      gl.enableVertexAttribArray(i + 1);
      gl.vertexAttribPointer(i + 1, size, gl.FLOAT, false, FLOATS * 4, offset * 4);
      gl.vertexAttribDivisor(i + 1, 1);
    });

    this.ramp = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.ramp);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  }

  /** Packs a segment; `authorSlot`/`langSlot` map owners and languages to palette slots. */
  upload(data: TreemapData, authorSlot: (owner: number) => number, langSlot: (lang: number) => number) {
    const n = data.ids.length;
    const buf = new Float32Array(n * FLOATS);
    for (let i = 0; i < n; i++) {
      const o = i * FLOATS;
      buf.set(data.geom.subarray(i * 8, i * 8 + 8), o);
      buf[o + 8] = data.churn[i * 2];
      buf[o + 9] = data.churn[i * 2 + 1];
      buf[o + 10] = authorSlot(data.owner[i * 2]);
      buf[o + 11] = authorSlot(data.owner[i * 2 + 1]);
      buf[o + 12] = langSlot(data.lang[i]);
      buf[o + 13] = data.flags[i];
    }
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    gl.bufferData(gl.ARRAY_BUFFER, buf, gl.STATIC_DRAW);
    this.count = n;
  }

  setPalette(p: Palette) {
    this.palette = p;
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.ramp);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, RAMP_SIZE, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, p.ramp);
  }

  draw(s: DrawState) {
    const { gl, palette: p } = this;
    if (!p) return;
    const canvas = gl.canvas as HTMLCanvasElement;
    const [w, h] = [Math.round(s.width * s.dpr), Math.round(s.height * s.dpr)];
    if (canvas.width !== w || canvas.height !== h) [canvas.width, canvas.height] = [w, h];
    gl.viewport(0, 0, w, h);
    const unit = (c: number[]) => c.map((v) => v / 255);
    gl.clearColor(...(unit(p.surface) as [number, number, number]), 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!this.count) return;

    gl.useProgram(this.program);
    const u = this.uniforms;
    gl.uniform1f(u.u_t, s.t);
    gl.uniform2f(u.u_view, s.width, s.height);
    gl.uniform3f(u.u_camera, s.camera.k, s.camera.x, s.camera.y);
    gl.uniform1i(u.u_mode, MODES[s.mode]);
    gl.uniform1f(u.u_spot, s.spot);
    gl.uniform1f(u.u_log_max, Math.log1p(CHURN_MAX));
    gl.uniform3fv(u.u_slots, p.categorical.flatMap(unit));
    gl.uniform3fv(u.u_zero, unit(p.zero));
    gl.uniform3fv(u.u_dir, unit(p.dir));
    gl.uniform3fv(u.u_ink, unit(p.ink));
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.ramp);
    gl.uniform1i(u.u_ramp, 0);
    gl.bindVertexArray(this.vao);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.count);
  }

  dispose() {
    this.gl.getExtension('WEBGL_lose_context')?.loseContext();
  }
}
