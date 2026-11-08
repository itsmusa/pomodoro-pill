import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Phase = "focus" | "break";
type Status = "ready" | "running" | "paused";

interface Snapshot {
  phase: Phase;
  status: Status;
  remaining_secs: number;
  total_secs: number;
  warn: boolean;
}

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) {
    throw new Error(`missing element #${id}`);
  }
  return found as T;
}

const pill = element<HTMLDivElement>("pill");
const timeEl = element<HTMLSpanElement>("time");
const fillEl = element<HTMLDivElement>("fill");
const labelSubEl = element<HTMLSpanElement>("label-sub");
const btnMain = element<HTMLButtonElement>("btn-main");
const btnStop = element<HTMLButtonElement>("btn-stop");
const btnStart = element<HTMLButtonElement>("btn-start");

function format(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`;
}

let armedTimer: number | undefined;

function setArmed(armed: boolean): void {
  window.clearTimeout(armedTimer);
  pill.dataset.armed = armed ? "true" : "false";
  btnStop.setAttribute(
    "aria-label",
    armed ? "Confirm stop" : pill.dataset.phase === "break" ? "Skip" : "Stop",
  );
  if (armed) {
    armedTimer = window.setTimeout(() => setArmed(false), 2000);
  }
}

function render(snapshot: Snapshot): void {
  pill.dataset.phase = snapshot.phase;
  pill.dataset.status = snapshot.status;
  pill.dataset.warn = String(snapshot.warn);

  timeEl.textContent = format(snapshot.remaining_secs);
  labelSubEl.textContent = `${format(snapshot.remaining_secs)} ready`;

  const progress =
    snapshot.total_secs > 0
      ? (1 - snapshot.remaining_secs / snapshot.total_secs) * 100
      : 0;
  fillEl.style.width = `${Math.min(100, Math.max(0, progress))}%`;

  btnMain.setAttribute(
    "aria-label",
    snapshot.status === "paused" ? "Resume" : "Pause",
  );
  btnStop.setAttribute(
    "aria-label",
    pill.dataset.armed === "true"
      ? "Confirm stop"
      : snapshot.phase === "break"
        ? "Skip"
        : "Stop",
  );

  if (snapshot.phase !== "focus" || snapshot.status === "ready") {
    setArmed(false);
  }
}

let flashTimer: number | undefined;

function flash(phase: Phase): void {
  window.clearTimeout(flashTimer);
  pill.dataset.flash = phase;
  pill.classList.remove("flash");
  void pill.offsetWidth;
  pill.classList.add("flash");
  flashTimer = window.setTimeout(() => {
    pill.classList.remove("flash");
    delete pill.dataset.flash;
  }, 600);
}

async function run(command: string): Promise<void> {
  try {
    render(await invoke<Snapshot>(command));
  } catch (error) {
    console.error(`${command} failed`, error);
  }
}

let hoverTimer: number | undefined;

pill.addEventListener("mouseenter", () => {
  window.clearTimeout(hoverTimer);
  pill.classList.add("hover");
});

pill.addEventListener("mouseleave", () => {
  window.clearTimeout(hoverTimer);
  hoverTimer = window.setTimeout(() => pill.classList.remove("hover"), 300);
});

btnMain.addEventListener("click", () => {
  const status = pill.dataset.status;
  if (status === "paused") {
    void run("resume");
  } else if (status === "running") {
    void run("pause");
  }
});

btnStart.addEventListener("click", () => {
  void run("start");
});

btnStop.addEventListener("click", () => {
  if (pill.dataset.phase === "break") {
    void run("skip");
    return;
  }
  if (pill.dataset.armed === "true") {
    setArmed(false);
    void run("stop");
  } else {
    setArmed(true);
  }
});

let saveTimer: number | undefined;

void getCurrentWindow()
  .onMoved(() => {
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      void invoke("save_position").catch((error) => {
        console.error("save_position failed", error);
      });
    }, 300);
  })
  .catch((error) => {
    console.error("onMoved failed", error);
  });

void listen<Snapshot>("engine://snapshot", (event) => {
  render(event.payload);
}).catch((error) => {
  console.error("snapshot listen failed", error);
});

void listen<Phase>("engine://phase_ended", (event) => {
  flash(event.payload);
}).catch((error) => {
  console.error("phase listen failed", error);
});

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    void run("get_snapshot");
  }
});

void run("get_snapshot");
