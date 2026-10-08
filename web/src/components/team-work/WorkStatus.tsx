import {
  CircleCheck,
  CirclePause,
  CircleHelp,
  Hand,
  LoaderCircle,
  OctagonAlert,
  Clock3,
} from 'lucide-react';
import { signalLabels, type WorkSignal } from '../../lib/workSignals';

const icons = {
  working: LoaderCircle,
  needs_you: Hand,
  blocked: OctagonAlert,
  done: CircleCheck,
  waiting: Clock3,
  stopped: CirclePause,
  unknown: CircleHelp,
};
export function WorkStatus({ signal, label }: { signal: WorkSignal; label?: string }) {
  const Icon = icons[signal];
  return (
    <span className="work-status" data-signal={signal}>
      <Icon size={13} aria-hidden="true" />
      {label || signalLabels[signal]}
    </span>
  );
}
