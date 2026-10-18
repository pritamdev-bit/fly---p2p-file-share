import { formatBytes } from "../api";

interface Props {
  done: number;
  total: number;
}

export default function Progress({ done, total }: Props) {
  const pct = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
  return (
    <div
      className="progress"
      role="progressbar"
      aria-valuenow={pct}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div className="progress-track">
        <div className="progress-fill" style={{ width: `${pct}%` }} />
      </div>
      <div className="progress-meta">
        <span>{pct}%</span>
        <span>
          {formatBytes(done)} of {formatBytes(total)}
        </span>
      </div>
    </div>
  );
}
