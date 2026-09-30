// The brand light: a dome of dark with the light rising under it, drawn
// with WebGL. The front page draws it full-screen and moves it; the
// sidebar draws one still frame of it behind its foot, so both read as
// the same light.
//
// Where the light withdraws it leaves the page's own colours, read from
// the identity's tokens, so it ends on the page the reader chose, light
// or dark.

const FRAGMENT = `
precision highp float;
uniform vec2 uRes; uniform float uTime, uSeed, uInt, uSize, uCY, uHaze, uGrain, uProg, uWob, uRim;
uniform vec3 uPaper, uTint;
float hash(vec2 p){ vec3 p3 = fract(vec3(p.xyx)*.1031); p3 += dot(p3, p3.yzx+33.33); return fract((p3.x+p3.y)*p3.z); }
float vnoise(vec2 p){ vec2 i=floor(p), f=fract(p); vec2 u=f*f*(3.-2.*f);
  return mix(mix(hash(i),hash(i+vec2(1,0)),u.x), mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),u.x), u.y); }
float fbm(vec2 p){ float v=0., a=.5; mat2 m=mat2(1.6,1.2,-1.2,1.6);
  for(int i=0;i<5;i++){ v+=a*vnoise(p); p=m*p+3.1; a*=.5; } return v; }
void main(){
  vec2 uv = gl_FragCoord.xy/uRes;
  float aspect = uRes.x/uRes.y;
  vec2 p = vec2((uv.x-.5)*aspect, uv.y);
  float t = uTime;
  vec2 q = p*1.8;
  vec2 w = vec2(fbm(q + vec2(0., t*.045)), fbm(q + vec2(4.7,1.3) - vec2(t*.035, 0.)));
  float n = fbm(q*1.35 + 1.7*w + vec2(t*.018, -t*.022));
  float fine = fbm(p*7.5 + 2.2*w + vec2(0., t*.05));
  float rx = max(.56*aspect, .34)*uSize, ry = .47*uSize;
  float d = length((p - vec2(0., uCY)) / vec2(rx, ry));
  d += (n-.5)*.38*uHaze + .018*sin(t*.42) + .01*sin(t*.23 + p.x*3.);
  float g = smoothstep(.8, 1.62, d);
  g *= mix(1., .72 + .56*fine, .55*min(uHaze,1.5));
  g = clamp(g*uInt, 0., 1.);
  vec3 top = vec3(.010,.047,.140), mid = vec3(.022,.088,.245);
  vec3 blue = mix(mid, top, smoothstep(.35, 1., uv.y));
  vec2 b = (p - vec2(0., uCY+.04))/vec2(.38,.32);
  blue += vec3(.010,.028,.075)*exp(-dot(b,b));
  blue = mix(blue, vec3(.070,.235,.560), smoothstep(.00,.36,g));
  blue = mix(blue, vec3(.255,.500,.845), smoothstep(.30,.64,g));
  blue = mix(blue, vec3(.660,.790,.945), smoothstep(.58,.86,g));
  blue = mix(blue, vec3(.945,.970,1.00), smoothstep(.82,1.0,g));
  vec2 a = vec2(-.5*aspect - .15, -.2);
  float ra = length(p - a);
  float tint = exp(-pow(ra/1.05, 2.));
  vec3 L = mix(uPaper, uTint, .8*tint);
  float pr = uProg, bell = sin(3.14159*pr);
  float dist = 1. - p.y;
  float wob = fbm(vec2(p.x*2.4, dist*1.6 - t*.35) + 1.2*w + vec2(t*.25, 0.));
  float lobes = fbm(vec2(p.x*1.1 - t*.15, t*.12 + pr*1.5));
  float bx = p.x/(.5*aspect + .001); float bow = bx*bx;
  float edge = dist + ((wob-.5)*.5 + (lobes-.5)*.7 + .12*bow)*bell*uWob;
  float R = mix(1.02, -.4, pr);
  float soft = mix(.02, .2, pr*pr);
  float m = 1. - smoothstep(R - soft, R + soft, edge);
  float halo = exp(-max(edge - R, 0.)/.17) * (1.-m) * bell;
  vec3 Lh = mix(L, vec3(.69,.80,.95), .5*halo*uRim);
  float rim = exp(-max(R - edge, 0.)/.045) * m * bell;
  vec3 Bc = mix(blue, vec3(.82,.90,.99), .65*rim*uRim);
  vec3 col = mix(Lh, Bc, m);
  float gr = hash(gl_FragCoord.xy + uSeed*vec2(113.1, 71.7)) - .5;
  col += gr*uGrain*mix(.012, .03 + .075*g, m);
  gl_FragColor = vec4(col, 1.);
}`;

/** A colour token as it resolves on this page, in the 0–1 range. */
function tone(name) {
    const probe = document.createElement("span");
    probe.hidden = true;
    probe.style.color = `var(${name})`;
    document.body.append(probe);
    const channels = getComputedStyle(probe).color.match(/[\d.]+/g)?.map(Number) ?? [0, 0, 0];
    probe.remove();

    return channels.slice(0, 3).map((channel) => channel / 255);
}

/**
 * The light, drawn on a canvas with the given settings; null where WebGL
 * is not available. `settings` holds `int`, `size`, `cy`, `haze`,
 * `grain`, `speed`, `wob` and `rim`.
 */
export function light(canvas, settings) {
    const P = settings;
    const gl = canvas.getContext("webgl", { antialias: false, premultipliedAlpha: false });
    if (!gl) return null;

    const shader = (type, source) => {
        const made = gl.createShader(type);
        gl.shaderSource(made, source);
        gl.compileShader(made);
        return gl.getShaderParameter(made, gl.COMPILE_STATUS) ? made : null;
    };
    const vertex = shader(gl.VERTEX_SHADER, "attribute vec2 a; void main(){ gl_Position = vec4(a,0.,1.); }");
    const fragment = shader(gl.FRAGMENT_SHADER, FRAGMENT);
    if (!vertex || !fragment) return null;

    const program = gl.createProgram();
    gl.attachShader(program, vertex);
    gl.attachShader(program, fragment);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;

    gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const at = gl.getAttribLocation(program, "a");
    gl.enableVertexAttribArray(at);
    gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);

    const uniform = {};
    for (const name of ["uRes", "uTime", "uSeed", "uInt", "uSize", "uCY", "uHaze", "uGrain", "uProg", "uWob", "uRim", "uPaper", "uTint"]) {
        uniform[name] = gl.getUniformLocation(program, name);
    }

    // Resolution drops while frames run slow, down to half.
    let quality = 1;
    let spent = 0;
    let frames = 0;
    let time = 7;
    let seed = 0;
    let grain = 0;
    let paper = tone("--surface-page");
    let tint = tone("--accent-soft-bg");

    function draw(progress) {
        const ratio = Math.min(window.devicePixelRatio || 1, 1.5) * quality;
        const width = Math.round(canvas.clientWidth * ratio);
        const height = Math.round(canvas.clientHeight * ratio);
        if (canvas.width !== width || canvas.height !== height) {
            canvas.width = width;
            canvas.height = height;
            gl.viewport(0, 0, width, height);
        }
        gl.uniform2f(uniform.uRes, canvas.width, canvas.height);
        gl.uniform1f(uniform.uTime, time);
        gl.uniform1f(uniform.uSeed, seed);
        gl.uniform1f(uniform.uInt, P.int);
        gl.uniform1f(uniform.uSize, P.size);
        gl.uniform1f(uniform.uCY, P.cy);
        gl.uniform1f(uniform.uHaze, P.haze);
        gl.uniform1f(uniform.uGrain, P.grain);
        gl.uniform1f(uniform.uProg, progress);
        gl.uniform1f(uniform.uWob, P.wob);
        gl.uniform1f(uniform.uRim, P.rim);
        gl.uniform3f(uniform.uPaper, ...paper);
        gl.uniform3f(uniform.uTint, ...tint);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
    }

    function advance(seconds, moving) {
        spent += seconds;
        frames += 1;
        if (frames >= 30) {
            if (spent / frames > 0.022 && quality > 0.5) quality *= 0.8;
            spent = 0;
            frames = 0;
        }
        time += seconds * (P.speed + (moving ? 0.6 : 0));
        grain += seconds;
        if (P.speed > 0 && grain > 1 / 18) {
            seed = (seed + 1) % 97;
            grain = 0;
        }
    }

    /** Reads the page's colours again, once the appearance has changed. */
    function recolour() {
        paper = tone("--surface-page");
        tint = tone("--accent-soft-bg");
    }

    return { draw, advance, recolour };
}
