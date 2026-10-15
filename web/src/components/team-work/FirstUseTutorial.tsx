import { useEffect, useRef, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { MessageSquare, Network, Users } from 'lucide-react';
import './first-use-tutorial.css';

// Device/browser preference, deliberately independent of workspace and connection token.
// Do not version this key: an app update must not restart someone's introduction.
const seenKey = 'tetonic_startup_tutorial_seen';
let seenWithoutStorage = false;

function claimFirstVisit() {
  try {
    if (localStorage.getItem(seenKey)) return false;
    // Remember display, not completion: even refreshing mid-tour must not repeat it.
    localStorage.setItem(seenKey, '1');
  } catch {
    // Restricted storage must never prevent someone from entering the workspace.
    if (seenWithoutStorage) return false;
    seenWithoutStorage = true;
  }
  return true;
}

const steps = [
  {
    label: 'Your team',
    icon: Users,
    title: 'Your ideas. Your team.',
    description:
      'The Guide helps you think and coordinate. Your agents carry out the work—one small request or several efforts at once.',
    location: 'Agents · in the top bar',
    hint: 'Choose models, create or edit agents, and give them the tools, skills, and connections they need. Start with one; teams are optional.',
  },
  {
    label: 'Your idea',
    icon: MessageSquare,
    title: 'Start with a thought.',
    description:
      'Ask a question or think through an idea with the Guide. When you want agents to act, review its proposal and choose to start the work.',
    location: 'The Guide · chat at the bottom',
    hint: 'You can brainstorm without starting a team plan. Agree on the direction before sending your agents to work.',
  },
  {
    label: 'Your control',
    icon: Network,
    title: 'See progress. Stay in control.',
    description:
      'Follow work on the map. Open an item to inspect the result or stop its work. Needs you brings decisions to you; Usage shows your token allowance.',
    location: 'Your first step',
    hint: 'Connect the Guide to a model. Then add or edit the agents that will carry out your work.',
  },
];

export function FirstUseTutorial({
  enabled,
  onSetupGuide,
}: {
  enabled: boolean;
  onSetupGuide: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [step, setStep] = useState(0);
  const checked = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    // Wait until the preloader has actually released the workspace, not merely
    // until the engine's first response arrives behind it.
    if (!enabled || checked.current) return;
    checked.current = true;
    if (claimFirstVisit()) setOpen(true);
  }, [enabled]);
  useEffect(() => {
    if (open) heading.current?.focus();
  }, [open, step]);

  const current = steps[step];
  const Icon = current.icon;
  const last = step === steps.length - 1;
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Portal>
        <Dialog.Overlay className="startup-tour-shade" />
        <Dialog.Content
          className="startup-tour"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            heading.current?.focus();
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            requestAnimationFrame(() =>
              (
                document.getElementById('px-inspect-heading') ||
                document.getElementById('main-content')
              )?.focus({ preventScroll: true }),
            );
          }}
          onPointerDownOutside={(event) => event.preventDefault()}
        >
          <header className="startup-tour-header">
            <span>Welcome to Tetonic</span>
            <Dialog.Close className="startup-tour-skip">Skip tour</Dialog.Close>
          </header>
          <ol className="startup-tour-progress" aria-label="Quick start progress">
            {steps.map((item, index) => (
              <li key={item.label} aria-current={index === step ? 'step' : undefined}>
                <span aria-hidden="true">{index + 1}</span> {item.label}
              </li>
            ))}
          </ol>
          <div className="startup-tour-body" key={step}>
            <div className="startup-tour-icon" aria-hidden="true">
              <Icon size={28} strokeWidth={1.5} />
            </div>
            <Dialog.Title ref={heading} tabIndex={-1}>
              {current.title}
            </Dialog.Title>
            <Dialog.Description>{current.description}</Dialog.Description>
            <div className="startup-tour-hint">
              <strong>{current.location}</strong>
              <p>{current.hint}</p>
            </div>
          </div>
          <footer className="startup-tour-footer">
            {step > 0 ? (
              <button type="button" onClick={() => setStep(step - 1)}>
                Back
              </button>
            ) : (
              <span className="startup-tour-count">1 of 3 · a quick introduction</span>
            )}
            <div>
              {last && <Dialog.Close>Explore on my own</Dialog.Close>}
              <button
                type="button"
                className="startup-tour-primary"
                disabled={last && !enabled}
                onClick={() => {
                  if (last) {
                    setOpen(false);
                    onSetupGuide();
                  } else setStep(step + 1);
                }}
              >
                {last ? 'Set up the Guide' : 'Next'}
              </button>
            </div>
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
