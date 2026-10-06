import type { SharedWorkEntry } from '../../src/lib/workContext';

const entry = (
  id: string,
  author: string,
  time: string,
  content: string,
  projectId = 'portal',
  kind: SharedWorkEntry['kind'] = 'message',
): SharedWorkEntry => ({
  id,
  projectId,
  author,
  authorId: `example-${author.toLowerCase()}`,
  time,
  order: Number(time.replace(':', '')) || undefined,
  content,
  kind,
});

// Authored example transcript. Keep original contributions intact; project cards
// are a different projection and must not rewrite or replace these messages.
export function exampleBlackboard(
  step: number,
  pilotChosen: boolean,
  pilotOrder = 925,
): SharedWorkEntry[] {
  const messages = [
    entry(
      'brief',
      'Mira',
      '09:10',
      '**First-release scope**\n\nThe six interview notes point to three recurring problems: finding account details, updating settings, and understanding billing. Billing changes have different permissions, so they stay outside this first release.\n\n- Include account details and settings.\n- Keep the current account page available as a fallback.\n- Do not invite customers until the audience and exact invitation are reviewed.\n\nJun and Remy: use shared brief revision 2. If the implementation needs more scope, bring that back here.',
    ),
    entry(
      'research',
      'Noor',
      '09:12',
      'The research summary is ready. I’m preserving links back to each interview so a claim can be checked against what the customer actually said. I’ll keep organizing those sources while the rest of you work.',
    ),
    entry(
      'contract-start',
      'Jun',
      '09:15',
      'I’m finishing the account contract. **Remy can build settings in parallel** against the agreed fields. I’ll send the contract and change notes to Sage for review; I won’t hold onto it while doing unrelated error handling.',
    ),
    entry(
      'ui',
      'Remy',
      '09:17',
      'I have the shared brief and draft schema. The settings screen is underway. I still need the reviewed contract before combining the interface and backend. Accessibility review will use that assembled interface, not these early mocks.',
    ),
    entry(
      'audience',
      'Eden',
      '09:19',
      '**A choice for the operator**\n\nShould the first release reach 25 invited customers or everyone? I suggest a small pilot so we can learn before widening it.\n\nI can keep preparing the rollback plan. Leo can keep drafting notes. Only the invitation draft needs the audience choice right now. Choosing an audience is not permission to send invitations or deploy.',
    ),
    entry(
      'market',
      'Ari',
      '09:20',
      '**Market research question**\n\nWe’re comparing the evidence for two customer segments. Bea is checking the source material; Sol and Rae are preparing interview questions. This contributes to the same customer-experience goal as the portal, but it has its own team and pace.',
      'research',
    ),
    entry(
      'market-limit',
      'Bea',
      '09:22',
      'The strongest claims still need independent sources. Please treat the segment ranking as provisional. I’ll attach the sources before we prepare a recommendation.',
      'research',
    ),
    entry(
      'ops',
      'Ash',
      '09:24',
      'I’m investigating recurring service failures. Kai is checking the incident evidence while Em and Bo prepare a bounded fix proposal. Investigation can continue without permission to deploy a change.',
      'operations',
    ),
  ];
  if (step >= 1)
    messages.push(
      entry(
        'code-result',
        'GitHub',
        '09:26',
        '```json\n{\n  "change": 42,\n  "artifact": "account-contract-v2",\n  "state": "ready_for_review",\n  "deployed": false\n}\n```',
        'portal',
        'tool',
      ),
      entry(
        'review-request',
        'Jun',
        '09:27',
        '@Sage — contract v2 and change #42 are ready for your review. Check account fields, expired-session behavior, and compatibility with the current page. Send findings back against this version. I’m moving to independent error handling; please interrupt me only if a finding invalidates that work.',
      ),
      entry(
        'review-accepted',
        'Sage',
        '09:28',
        'Review request received. I’m reviewing the submitted contract, not the entire portal. Ivy: wait for my acceptance before running compatibility checks against this version. Your test preparation can continue.',
      ),
    );
  if (step >= 2)
    messages.push(
      entry(
        'review-result',
        'Sage',
        '09:32',
        '**Contract v2 accepted for compatibility testing.**\n\nAccount fields match the shared brief. The expired-session response has an explicit error case. This review does not certify the assembled interface or authorize a release.\n\n@Ivy — you can run the compatibility checks. @Jun — the separate error-handling work can continue.',
      ),
      entry(
        'tests-start',
        'Ivy',
        '09:33',
        'I have contract v2 and Sage’s review. Running the eight account compatibility checks now. Full-journey and accessibility checks are still waiting on Remy’s assembled interface.',
      ),
    );
  if (step >= 3)
    messages.push(
      entry(
        'test-result',
        'Test environment',
        '09:36',
        '```text\naccount-contract-v2\nchecks: 8\npassed: 8\nfailed: 0\nscope: API compatibility only\nnot evaluated: assembled UI, accessibility, full customer journey\n```',
        'portal',
        'tool',
      ),
      entry(
        'tests-done',
        'Ivy',
        '09:37',
        '@Eden — the compatibility report is attached. You can use it in rollout preparation, but the full release is not ready: Remy is still building the interface and the remaining checks must follow. Please preserve that distinction in the release summary.',
      ),
    );
  if (pilotChosen)
    messages.push(
      entry(
        'pilot-choice',
        'Operator',
        'After your choice',
        'Prepare the draft for 25 invited customers. This changes the audience scope only; sending invitations and deploying still require separate review.',
        'portal',
        'direction',
      ),
    );
  return messages.map((message) =>
    message.id === 'pilot-choice' ? { ...message, order: pilotOrder } : message,
  );
}
