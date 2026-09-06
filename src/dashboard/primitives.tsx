import type { ReactNode } from "react";

import type { DeviceStatus } from "../ipc";

export function Section(props: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="section">
      <h2 className="section__title">
        {props.title}
        {props.note && <span className="section__note">{props.note}</span>}
      </h2>
      {props.children}
    </section>
  );
}

/**
 * Status is icon + colour + word, always. Never colour alone — this is a
 * go/no-go interface and colourblind users exist.
 */
const STATUS: Record<DeviceStatus, { glyph: string; word: string; cls: string }> = {
  connected: { glyph: "●", word: "CONNECTED", cls: "pill--pass" },
  connecting: { glyph: "◐", word: "CONNECTING", cls: "pill--run" },
  disconnected: { glyph: "○", word: "DISCONNECTED", cls: "pill--idle" },
};

export function StatusPill({ status }: { status: DeviceStatus }) {
  const s = STATUS[status];
  return (
    <span className={`pill ${s.cls}`}>
      <span aria-hidden="true">{s.glyph}</span> {s.word}
    </span>
  );
}
