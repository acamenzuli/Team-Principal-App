import { formatBytes } from "./client";
import type { ModuleStatus, VoiceLabRequirements } from "../ipc";

/**
 * What the Voice Lab needs, and where this machine stands.
 *
 * Shown instead of the tab when the hardware does not qualify or the module
 * is not built. On unsupported hardware it explains rather than disappears —
 * a tab that vanishes looks like a bug, and somebody who has just read the
 * feature list deserves to know why it is not for them.
 */
export function Requirements({
  requirements,
  module,
  error,
  onInstall,
  onRemove,
}: {
  requirements: VoiceLabRequirements;
  module: ModuleStatus;
  error: string | null;
  onInstall: () => void;
  onRemove: () => void;
}) {
  const gpu = requirements.gpu;
  const partly = module.uvPresent || module.pythonPresent || module.envPresent;

  return (
    <section className="voicelab">
      <header className="voicelab__head">
        <h1 className="voicelab__title">Voice Lab</h1>
      </header>

      <div className="voicelab__intro">
        <p>
          Record about five minutes of your own voice and the Voice Lab turns it into a full
          CrewChief voice pack — the chief and the spotter — with every clip checked by a second
          model before it goes in.
        </p>
        <p className="note">
          It runs entirely on this machine. Nothing is uploaded, and your recordings never leave
          the app's own folder.
        </p>
      </div>

      <div className="group glass">
        <header className="group__head">
          <h2 className="group__title">This machine</h2>
          <p className="group__note">checked each time this tab opens</p>
        </header>
        <div className="group__body">
          <Check
            ok={!!gpu && gpu.vendor === "nvidia"}
            label="An NVIDIA graphics card"
            detail={gpu ? gpu.name : "none detected"}
          />
          <Check
            ok={!!gpu && gpu.vramMb >= requirements.minVramMb}
            label={`At least ${Math.round(requirements.minVramMb / 1024)} GB of video memory`}
            detail={gpu ? `${(gpu.vramMb / 1024).toFixed(1)} GB` : "—"}
          />
          <Check
            ok={!!gpu?.driver && requirements.problems.every((p) => !p.includes("driver"))}
            label={`NVIDIA driver ${requirements.minDriver} or newer`}
            detail={gpu?.driver ? `${gpu.driver}` : "could not be read"}
          />
          {requirements.gpus.length > 1 && (
            <p className="note">
              {requirements.gpus.length} adapters found; the Voice Lab would use {gpu?.name}.
            </p>
          )}
        </div>
      </div>

      {!requirements.ok && (
        <div className="group glass">
          <header className="group__head">
            <h2 className="group__title">Not on this machine</h2>
          </header>
          <div className="group__body">
            {requirements.problems.map((problem) => (
              <p key={problem} className="note note--fail">
                <span aria-hidden="true">■</span> {problem}
              </p>
            ))}
            <p className="note">
              Everything else in Team Principal works exactly as it does now. This is the only part
              that needs a graphics card.
            </p>
          </div>
        </div>
      )}

      {requirements.ok && (
        <div className="group glass">
          <header className="group__head">
            <h2 className="group__title">The Voice Lab download</h2>
            <p className="group__note">about 14 GB, kept out of the installer</p>
          </header>
          <div className="group__body">
            <p className="note">
              Python, PyTorch and the speech models are several gigabytes, and most people will
              never open this tab — so they are not in the installer. Downloading puts them in the
              app's own folder, and <strong>Remove Voice Lab</strong> takes every byte back.
            </p>
            <p className="note num">{module.home}</p>

            {module.busy ? (
              <>
                <p className="voicelab__stage">{module.message}</p>
                <progress className="voicelab__progress" />
                <details className="voicelab__log">
                  <summary className="note">What it is doing</summary>
                  <pre className="num">{module.log.join("\n")}</pre>
                </details>
              </>
            ) : (
              <>
                <div className="settings__actions">
                  <button className="btn" onClick={onInstall}>
                    {partly ? "Carry on downloading" : "Download the Voice Lab"}
                  </button>
                  {partly && (
                    <button className="btn btn--quiet" onClick={onRemove}>
                      Remove what was downloaded ({formatBytes(module.bytesOnDisk)})
                    </button>
                  )}
                </div>
                {partly && !module.error && (
                  <p className="note">
                    Part of it is already here. Carrying on picks up where it stopped rather than
                    starting again.
                  </p>
                )}
                {module.message && !module.error && <p className="note">{module.message}</p>}
              </>
            )}

            {(module.error || error) && (
              <p className="note note--fail">
                <span aria-hidden="true">■</span> {module.error ?? error}
              </p>
            )}
          </div>
        </div>
      )}

      <div className="group glass">
        <header className="group__head">
          <h2 className="group__title">What gets downloaded</h2>
        </header>
        <div className="group__body voicelab__licences">
          <Row name="CPython 3.11" from="python.org, via uv" licence="PSF" />
          <Row name="PyTorch (CUDA 12.6)" from="pytorch.org" licence="BSD-3-Clause" />
          <Row name="Chatterbox" from="Resemble AI, on Hugging Face" licence="MIT" />
          <Row name="Whisper large-v3-turbo" from="Hugging Face" licence="MIT" />
          <Row name="Silero VAD" from="PyPI" licence="MIT" />
          <p className="note">
            Everything here is licensed for use in a product that is sold. Each model comes from
            its own publisher, over HTTPS; nothing is re-hosted.
          </p>
        </div>
      </div>
    </section>
  );
}

function Check({ ok, label, detail }: { ok: boolean; label: string; detail: string }) {
  return (
    <div className="voicelab__check">
      <span className={ok ? "status status--pass" : "status status--fail"}>
        <span aria-hidden="true">{ok ? "●" : "■"}</span> {ok ? "OK" : "NO"}
      </span>
      <span>{label}</span>
      <span className="note num">{detail}</span>
    </div>
  );
}

function Row({ name, from, licence }: { name: string; from: string; licence: string }) {
  return (
    <div className="voicelab__licence">
      <span>{name}</span>
      <span className="note">{from}</span>
      <span className="note num">{licence}</span>
    </div>
  );
}
