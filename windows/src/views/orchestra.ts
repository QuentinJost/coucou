// Orchestra panel — the tasks of the Claude Code orchestra (`/orch`), beside the
// overview. Read only: what the orchestrator's queue files say, nothing more.

import { h, clear, dot } from "./dom";
import { State, type OrchTask } from "../core/state";
import { ORCH_ROWS } from "../core/layout";

const STATE_LOOK: Record<OrchTask["state"], { color: string; label: string }> = {
  working: { color: "#3B9EFF", label: "working" },
  checking: { color: "#A78BFA", label: "checks" },
  reviewing: { color: "#22D3EE", label: "review" },
  queued: { color: "#6B7079", label: "queued" },
  blocked: { color: "#F4505E", label: "blocked" },
  escalated: { color: "#F5A524", label: "escalated" },
  approved: { color: "#34D399", label: "approved" },
};

const look = (state: OrchTask["state"]) => STATE_LOOK[state] ?? { color: "#6B7079", label: state };

export interface OrchestraPanel {
  el: HTMLElement;
  sync(): void;
}

export function buildOrchestra(): OrchestraPanel {
  const counts = h("span", { class: "orch-counts" });
  const rows = h("div", { class: "orch-rows" });
  const el = h(
    "div",
    { id: "orchestra" },
    h("div", { class: "orch-head" }, h("span", { class: "orch-title", text: "Orchestra" }), counts),
    h("div", { class: "card orch-card" }, rows),
  );

  let key = "";

  return {
    el,
    sync() {
      const shown = State.orchShown;
      el.classList.toggle("on", shown);
      if (!shown) return;

      const tasks = State.orchestra;
      const next = JSON.stringify(tasks);
      if (next === key) return;
      key = next;

      const busy = tasks.filter((t) => ["working", "checking", "reviewing"].includes(t.state)).length;
      const waiting = tasks.filter((t) => t.state === "blocked" || t.state === "escalated").length;
      const queued = tasks.filter((t) => t.state === "queued").length;
      counts.textContent = [
        busy ? `${busy} active` : "",
        waiting ? `${waiting} for you` : "",
        queued ? `${queued} queued` : "",
      ].filter(Boolean).join(" · ");

      clear(rows);
      for (const t of tasks.slice(0, ORCH_ROWS)) rows.append(buildRow(t));
      if (tasks.length > ORCH_ROWS) {
        rows.append(h("div", { class: "orch-more", text: `+ ${tasks.length - ORCH_ROWS} more` }));
      }
    },
  };
}

function buildRow(t: OrchTask): HTMLElement {
  const { color, label } = look(t.state);
  const state = t.round > 1 && t.state !== "approved" ? `${label} · r${t.round}` : label;
  const who = [t.session, t.model].filter(Boolean).join(" · ");
  const row = h(
    "div",
    { class: "orch-row", title: [`#${t.id} ${t.title}`, who, `orchestrator: ${t.orchestrator}`].filter(Boolean).join("\n") },
    h(
      "div",
      { class: "orch-line" },
      dot(color, 7),
      h("span", { class: "orch-id", text: `#${t.id}` }),
      h("span", { class: "orch-role", text: t.role }),
      h("span", { class: "orch-repo", text: t.repo }),
      h("span", { class: "orch-state", text: state }),
    ),
    h("div", { class: "orch-task", text: t.title }),
  );
  (row.querySelector(".orch-state") as HTMLElement).style.color = color;
  if (["working", "checking", "reviewing"].includes(t.state)) row.classList.add("busy");
  return row;
}
