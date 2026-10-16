// Thin client: renders server state and sends intents. All rules live on the
// server; untrusted values are only ever written with textContent.
"use strict";

const $ = (id) => document.getElementById(id);
const boardEl = $("board");
const cells = [];
let ws = null;
let state = null;
let retryMs = 500;
let errorTimer = null;

for (let i = 0; i < 9; i++) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "cell";
  b.setAttribute("role", "gridcell");
  b.addEventListener("click", () => send({ type: "move", cell: i }));
  boardEl.appendChild(b);
  cells.push(b);
}

// Keyboard: 1-9 like a phone keypad laid over the board; n = new game.
document.addEventListener("keydown", (e) => {
  if (e.target instanceof HTMLInputElement) return;
  if (e.key >= "1" && e.key <= "9") send({ type: "move", cell: Number(e.key) - 1 });
  else if (e.key === "n") send({ type: "new_game" });
  else if (e.key === "d" && state) {
    const order = ["easy", "medium", "hard"];
    send({ type: "set_difficulty", difficulty: order[(order.indexOf(state.difficulty) + 1) % 3] });
  }
});

$("new-game").addEventListener("click", () => send({ type: "new_game" }));
document.querySelectorAll('input[name="ai"]').forEach((r) =>
  r.addEventListener("change", () => send({ type: "set_ai", player: r.value || null }))
);
document.querySelectorAll('input[name="difficulty"]').forEach((r) =>
  r.addEventListener("change", () => send({ type: "set_difficulty", difficulty: r.value }))
);

function send(msg) {
  if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
}

function showError(text) {
  $("error").textContent = text;
  clearTimeout(errorTimer);
  errorTimer = setTimeout(() => ($("error").textContent = ""), 2500);
}

function render(s) {
  state = s;
  const over = s.status.kind !== "in_progress";
  const aiTurn = !over && s.ai === s.turn;
  const winLine = s.status.kind === "won" ? s.status.line : [];
  const last = s.history.length ? s.history[s.history.length - 1] : -1;

  s.board.forEach((v, i) => {
    const c = cells[i];
    c.textContent = v ?? "";
    c.className = "cell" + (v ? ` cell--${v.toLowerCase()}` : "") +
      (winLine.includes(i) ? " cell--win" : "") + (i === last ? " cell--last" : "");
    c.disabled = over || aiTurn || v !== null;
    c.setAttribute("aria-label", `cell ${i + 1}: ${v ?? "empty"}`);
  });

  let status;
  if (s.status.kind === "won") status = `${s.status.winner} wins!` + (s.ai === s.status.winner ? " (AI)" : "");
  else if (s.status.kind === "draw") status = "Draw.";
  else status = aiTurn ? `AI (${s.turn}) is thinking…` : `${s.turn} to move`;
  $("status").textContent = status;

  $("score-x").textContent = s.score.x;
  $("score-o").textContent = s.score.o;
  $("score-d").textContent = s.score.draws;
  $("game-no").textContent = s.game_no;
  $("clients").textContent = s.web_clients;
  $("history").textContent = s.history.length ? s.history.map((c) => c + 1).join(" → ") : "–";
  document.querySelectorAll('input[name="ai"]').forEach((r) => (r.checked = r.value === (s.ai ?? "")));
  document.querySelectorAll('input[name="difficulty"]').forEach((r) => (r.checked = r.value === s.difficulty));
  // Level only matters when an AI is playing.
  $("level").classList.toggle("mode--off", s.ai === null);
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws`);
  ws.addEventListener("open", () => {
    retryMs = 500;
    $("conn").textContent = "live";
    $("conn").className = "conn conn--up";
  });
  ws.addEventListener("message", (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch { return; }
    if (msg.type === "state") render(msg);
    else if (msg.type === "error") showError(msg.message);
  });
  ws.addEventListener("close", () => {
    $("conn").textContent = "reconnecting…";
    $("conn").className = "conn conn--down";
    setTimeout(connect, retryMs);
    retryMs = Math.min(retryMs * 2, 5000);
  });
}

connect();
