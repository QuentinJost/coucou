// Orchestra panel — the tasks of the Claude Code orchestra (`/orch`), beside the
// overview. Read only: what the orchestrator's queue files say, nothing more.

import { h, clear, dot } from "./dom";
import { N_, t, tl } from "../i18n/i18n";
import { State, type OrchTask } from "../core/state";
import { ORCH_ROWS } from "../core/layout";

// Labels are i18n keys, translated where they are shown.
const STATE_LOOK: Record<OrchTask["state"], { color: string; label: string }> = {
  working: { color: "#3B9EFF", label: N_("Working") },
  checking: { color: "#A78BFA", label: N_("Checks") },
  reviewing: { color: "#22D3EE", label: N_("Review") },
  queued: { color: "#6B7079", label: N_("Queued") },
  blocked: { color: "#F4505E", label: N_("Blocked") },
  escalated: { color: "#F5A524", label: N_("Your call") },
  approved: { color: "#34D399", label: N_("Approved") },
};

const look = (state: OrchTask["state"]) => {
  const found = STATE_LOOK[state];
  return found ? { color: found.color, label: t(found.label) } : { color: "#6B7079", label: state };
};

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
    h("div", { class: "orch-head" }, h("span", { class: "orch-title", text: tl("Orchestra") }), counts),
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

      const busy = tasks.filter((task) => ["working", "checking", "reviewing"].includes(task.state)).length;
      const waiting = tasks.filter((task) => task.state === "blocked" || task.state === "escalated").length;
      const queued = tasks.filter((task) => task.state === "queued").length;
      counts.textContent = [
        busy ? t("Active: {count}", { count: busy }) : "",
        waiting ? t("For you: {count}", { count: waiting }) : "",
        queued ? t("Queued: {count}", { count: queued }) : "",
      ].filter(Boolean).join(" · ");

      clear(rows);
      for (const task of tasks.slice(0, ORCH_ROWS)) rows.append(buildRow(task));
      if (tasks.length > ORCH_ROWS) {
        rows.append(h("div", { class: "orch-more", text: t("+ {count} more", { count: tasks.length - ORCH_ROWS }) }));
      }
    },
  };
}

function buildRow(task: OrchTask): HTMLElement {
  const { color, label } = look(task.state);
  const state = task.round > 1 && task.state !== "approved" ? `${label} · ${t("round {count}", { count: task.round })}` : label;
  const who = [task.session, task.model].filter(Boolean).join(" · ");
  const row = h(
    "div",
    { class: "orch-row", title: [`#${task.id} ${task.title}`, who, t("orchestrator: {name}", { name: task.orchestrator })].filter(Boolean).join("\n") },
    h(
      "div",
      { class: "orch-line" },
      dot(color, 7),
      h("span", { class: "orch-id", text: `#${task.id}` }),
      h("span", { class: "orch-role", text: task.role }),
      h("span", { class: "orch-repo", text: task.repo }),
      h("span", { class: "orch-state", text: state }),
    ),
    h("div", { class: "orch-task", text: task.title }),
  );
  (row.querySelector(".orch-state") as HTMLElement).style.color = color;
  if (["working", "checking", "reviewing"].includes(task.state)) row.classList.add("busy");
  return row;
}
