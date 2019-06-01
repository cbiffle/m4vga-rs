import { Tunnel, Rotozoom } from "m4vga-wasm-demos";
import * as wasm from "m4vga-wasm-demos";
import { memory } from "m4vga-wasm-demos/m4vga_wasm_demos_bg";

const demos = {
  "tunnel": Tunnel,
  "rotozoom": Rotozoom,
};

var demo = null;
const width = wasm.width();
const height = wasm.height();

var ptr = null;
var buffer = null;
var image = null;

const activate = (name) => {
  demo = demos[name].new();
  ptr = demo.framebuffer();
  buffer = new Uint8ClampedArray(memory.buffer, ptr, 4 * width * height);
  image = new ImageData(buffer, width);
};

const canvas = document.getElementById("demo-canvas");
canvas.height = height;
canvas.width = width;

const playPauseButton = document.getElementById("run-pause");

const play = () => {
  playPauseButton.textContent = "⏸";
  document.getElementById("single-step").disabled = true;
  renderLoop();
};

const pause = () => {
  playPauseButton.textContent = "▶";
  cancelAnimationFrame(animationId);
  animationId = null;
  document.getElementById("single-step").disabled = false;
};

const isPaused = () => {
  return animationId === null;
};

playPauseButton.addEventListener("click", event => {
  if (isPaused()) {
    play();
  } else {
    pause();
  }
});

document.getElementById("single-step").addEventListener("click", event => {
  demo.step();
  drawFramebuffer();
});

const demoSelect = document.getElementById("choose-demo");
for (let d in demos) {
  console.log(d);
  let opt = document.createElement("option");
  opt.text = d;
  demoSelect.options.add(opt);
}
demoSelect.addEventListener("change", event => {
  let name = demoSelect.options[demoSelect.selectedIndex].text;
  console.log(name);
  activate(name);
});

const ctx = canvas.getContext('2d');

let animationId = null;

const renderLoop = () => {
  demo.step();

  drawFramebuffer();

  animationId = requestAnimationFrame(renderLoop);
};

const drawFramebuffer = () => {
  ctx.putImageData(image, 0, 0);
};

activate("tunnel");
play();
