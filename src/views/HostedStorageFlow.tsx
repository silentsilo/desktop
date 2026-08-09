import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckCircle2, ShieldQuestion } from "lucide-react";

type PairingStart = {
  user_code: string;
  device_code: string;
  verification_url: string;
  expires_in: number;
  interval: number;
};

type HostedDisplay = {
  account: string;
  label: string;
  plan: string;
  region: string;
  portalUrl: string;
};

type Poll =
  | { status: "pending" }
  | { status: "slow_down" }
  | { status: "expired" }
  | { status: "cancelled" }
  | { status: "approved"; display: HostedDisplay };

type Phase =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "waiting"; pairing: PairingStart }
  | { kind: "confirm"; display: HostedDisplay }
  | { kind: "saving" }
  | { kind: "done" }
  | { kind: "error"; message: string };

/// How long the code lives, said from the server's own number rather than
/// hardcoded: the wording stays honest if the service changes its window.
function describeExpiry(seconds: number): string {
  const mins = Math.round(seconds / 60);
  return `The code lasts ${mins} minute${mins === 1 ? "" : "s"}`;
}

export function HostedStorageFlow({
  onConnected,
  onCancel,
  additional,
  confirmHere,
}: {
  onConnected: () => void;
  onCancel: () => void;
  /** Extra silo configuration passed down to `confirm` when confirmed in app. */
  additional?: Record<string, unknown>;
  /**
   * Whether to confirm right in this component (`JoinView`) or to stay
   * pending for the caller to confirm (`HostedStorageFlow`).
   */
  confirmHere?: boolean;
}) {
  const [phase, setPhase] = useState<Phase>({ kind: "starting" });

  useEffect(() => {
    let mounted = true;
    let timerId: number | null = null;

    async function init() {
      setPhase({ kind: "starting" });
      try {
        const pairing = await invoke<PairingStart>("hosted_pair_start");
        if (!mounted) return;

        setPhase({ kind: "waiting", pairing });
        void openUrl(`${pairing.verification_url}?code=${encodeURIComponent(pairing.user_code)}`).catch(() => undefined);

        let interval = Math.max(1, pairing.interval) * 1000;
        const deadline = Date.now() + pairing.expires_in * 1000;

        const tick = async () => {
          if (!mounted) return;
          if (Date.now() > deadline) {
            if (timerId !== null) window.clearInterval(timerId);
            setPhase({ kind: "error", message: "That code expired. Start again when you are ready." });
            return;
          }
          try {
            const poll = await invoke<Poll>("hosted_pair_poll", { deviceCode: pairing.device_code });
            if (!mounted) return;

            if (poll.status === "approved") {
              if (timerId !== null) window.clearInterval(timerId);
              setPhase({ kind: "confirm", display: poll.display });
            } else if (poll.status === "expired" || poll.status === "cancelled") {
              if (timerId !== null) window.clearInterval(timerId);
              setPhase({
                kind: "error",
                message:
                  poll.status === "cancelled"
                    ? "That pairing was cancelled."
                    : "That code expired. Start again when you are ready.",
              });
            } else if (poll.status === "slow_down") {
              if (timerId !== null) window.clearInterval(timerId);
              interval = Math.min(interval * 2, 15000);
              timerId = window.setInterval(() => void tick(), interval);
            }
          } catch (error) {
            if (!mounted) return;
            if (timerId !== null) window.clearInterval(timerId);
            setPhase({ kind: "error", message: String(error) });
          }
        };

        // Run immediate tick on start, then set recurring interval
        void tick();
        timerId = window.setInterval(() => void tick(), interval);
      } catch (error) {
        if (mounted) setPhase({ kind: "error", message: String(error) });
      }
    }

    void init();

    return () => {
      mounted = false;
      if (timerId !== null) window.clearInterval(timerId);
    };
  }, []);

  const confirm = useCallback(async () => {
    if (phase.kind !== "confirm") return;
    setPhase({ kind: "saving" });
    try {
      if (confirmHere) await invoke("hosted_pair_confirm", { additional: Boolean(additional) });
      setPhase({ kind: "done" });
      onConnected();
    } catch (error) {
      setPhase({ kind: "error", message: String(error) });
    }
  }, [phase, onConnected, additional, confirmHere]);

  const reject = useCallback(async () => {
    await invoke("hosted_pair_cancel").catch(() => undefined);
    setPhase({ kind: "idle" });
    onCancel();
  }, [onCancel]);

  if (phase.kind === "confirm") {
    return (
      <div className="hosted-confirm">
        <h4>
          <ShieldQuestion size={16} />
          Is this your account?
        </h4>
        <dl className="hosted-facts">
          <div>
            <dt>Account</dt>
            <dd>{phase.display.account}</dd>
          </div>
          <div>
            <dt>Plan</dt>
            <dd>{phase.display.plan}</dd>
          </div>
          <div>
            <dt>Region</dt>
            <dd>{phase.display.region}</dd>
          </div>
          <div>
            <dt>Silo name</dt>
            <dd>{phase.display.label}</dd>
          </div>
        </dl>

        <p className="muted small">
          Data under this silo will be encrypted for your key alone. The service
          sees only ciphertext and cannot read any of it.
        </p>

        <div className="hosted-actions">
          <button type="button" onClick={() => void confirm()}>
            Yes, connect this silo
          </button>
          <button type="button" className="secondary" onClick={() => void reject()}>
            No, cancel
          </button>
        </div>
      </div>
    );
  }

  if (phase.kind === "saving") {
    return (
      <div className="hosted-waiting">
        <p>Connecting your silo...</p>
      </div>
    );
  }

  if (phase.kind === "done") {
    return (
      <div className="hosted-done">
        <CheckCircle2 size={32} style={{ color: "var(--emerald)" }} />
        <h3>Connected</h3>
      </div>
    );
  }

  if (phase.kind === "error") {
    return (
      <div className="hosted-error">
        <p>{phase.message}</p>
        <button type="button" className="secondary" onClick={onCancel}>
          Try again
        </button>
      </div>
    );
  }

  return (
    <div className="hosted-waiting">
      <p>Finish in your browser. If it did not open, go to {phase.kind === "waiting" ? phase.pairing.verification_url : ""}</p>
      {phase.kind === "waiting" ? (
        <>
          <p className="hosted-code" aria-label="Your pairing code">
            {phase.pairing.user_code}
          </p>
          <p className="muted small">
            {describeExpiry(phase.pairing.expires_in)}.
          </p>
        </>
      ) : (
        <p className="muted small">Generating pairing code...</p>
      )}

      <div className="hosted-actions" style={{ marginTop: "1rem" }}>
        <button type="button" className="secondary" onClick={() => void reject()}>
          Cancel
        </button>
      </div>
    </div>
  );
}
