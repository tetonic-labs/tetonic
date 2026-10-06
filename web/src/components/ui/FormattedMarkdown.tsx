import React from 'react';

interface FormattedMarkdownProps {
  content?: string;
  text?: string;
  className?: string;
}

export function FormattedMarkdown({ content, text, className = '' }: FormattedMarkdownProps) {
  const markdownText = content ?? text ?? '';
  if (!markdownText) return null;

  // Split into lines to parse blocks
  const rawLines = markdownText.replace(/\r\n/g, '\n').split('\n');
  const elements: React.ReactNode[] = [];

  let inCodeBlock = false;
  let codeBlockContent: string[] = [];
  let codeBlockLang = '';
  let currentList: { type: 'ul' | 'ol'; items: string[] } | null = null;
  let paragraphLines: string[] = [];

  const flushParagraph = (key: string) => {
    if (paragraphLines.length > 0) {
      const text = paragraphLines.join(' ');
      elements.push(
        <p key={key} className="wr-md-p" style={{ margin: '8px 0', lineHeight: 1.65, fontSize: '15px' }}>
          {renderInline(text)}
        </p>
      );
      paragraphLines = [];
    }
  };

  const flushList = (key: string) => {
    if (currentList) {
      if (currentList.type === 'ul') {
        elements.push(
          <ul
            key={key}
            className="wr-md-ul"
            style={{
              margin: '8px 0 12px 20px',
              paddingLeft: '12px',
              listStyleType: 'disc',
              display: 'flex',
              flexDirection: 'column',
              gap: '4px',
            }}
          >
            {currentList.items.map((item, idx) => (
              <li key={idx} style={{ lineHeight: 1.6, fontSize: '14px' }}>
                {renderInline(item)}
              </li>
            ))}
          </ul>
        );
      } else {
        elements.push(
          <ol
            key={key}
            className="wr-md-ol"
            style={{
              margin: '8px 0 12px 20px',
              paddingLeft: '12px',
              listStyleType: 'decimal',
              display: 'flex',
              flexDirection: 'column',
              gap: '4px',
            }}
          >
            {currentList.items.map((item, idx) => (
              <li key={idx} style={{ lineHeight: 1.6, fontSize: '14px' }}>
                {renderInline(item)}
              </li>
            ))}
          </ol>
        );
      }
      currentList = null;
    }
  };

  for (let i = 0; i < rawLines.length; i++) {
    const line = rawLines[i];
    const trimmed = line.trim();

    // Check code blocks
    if (trimmed.startsWith('```')) {
      if (inCodeBlock) {
        // Close code block
        elements.push(
          <pre
            key={`code-${i}`}
            className="wr-md-codeblock"
            style={{
              background: 'rgba(20, 20, 25, 0.06)',
              border: '1px solid rgba(0, 0, 0, 0.1)',
              borderRadius: '6px',
              padding: '12px 14px',
              margin: '10px 0',
              fontFamily: 'monospace',
              fontSize: '13px',
              lineHeight: 1.5,
              overflowX: 'auto',
              whiteSpace: 'pre-wrap',
            }}
          >
            <code data-language={codeBlockLang || undefined}>{codeBlockContent.join('\n')}</code>
          </pre>
        );
        codeBlockContent = [];
        codeBlockLang = '';
        inCodeBlock = false;
      } else {
        flushParagraph(`p-${i}`);
        flushList(`list-${i}`);
        inCodeBlock = true;
        codeBlockLang = trimmed.slice(3).trim();
      }
      continue;
    }

    if (inCodeBlock) {
      codeBlockContent.push(line);
      continue;
    }

    // Empty lines
    if (trimmed === '') {
      flushParagraph(`p-${i}`);
      flushList(`list-${i}`);
      continue;
    }

    // Headers
    if (trimmed.startsWith('### ')) {
      flushParagraph(`p-${i}`);
      flushList(`list-${i}`);
      elements.push(
        <h4
          key={`h4-${i}`}
          className="wr-md-h4"
          style={{ fontSize: '15px', fontWeight: 600, margin: '14px 0 6px 0', color: 'inherit' }}
        >
          {renderInline(trimmed.slice(4))}
        </h4>
      );
      continue;
    }
    if (trimmed.startsWith('## ')) {
      flushParagraph(`p-${i}`);
      flushList(`list-${i}`);
      elements.push(
        <h3
          key={`h3-${i}`}
          className="wr-md-h3"
          style={{ fontSize: '17px', fontWeight: 600, margin: '16px 0 6px 0', color: 'inherit' }}
        >
          {renderInline(trimmed.slice(3))}
        </h3>
      );
      continue;
    }
    if (trimmed.startsWith('# ')) {
      flushParagraph(`p-${i}`);
      flushList(`list-${i}`);
      elements.push(
        <h2
          key={`h2-${i}`}
          className="wr-md-h2"
          style={{ fontSize: '19px', fontWeight: 600, margin: '18px 0 8px 0', color: 'inherit' }}
        >
          {renderInline(trimmed.slice(2))}
        </h2>
      );
      continue;
    }

    // Bold header standalone like `**Directories (10):**`
    if (/^\*\*[^*]+\*\*:\s*$/.test(trimmed) || /^\*\*[^*]+\*\*\s*$/.test(trimmed)) {
      flushParagraph(`p-${i}`);
      flushList(`list-${i}`);
      const text = trimmed.replace(/^\*\*/, '').replace(/\*\*[:\s]*$/, '');
      elements.push(
        <h4
          key={`bold-h4-${i}`}
          className="wr-md-h4"
          style={{ fontSize: '15px', fontWeight: 600, margin: '14px 0 6px 0', color: 'inherit' }}
        >
          {text}
        </h4>
      );
      continue;
    }

    // Unordered list items: `- item` or `* item`
    const ulMatch = trimmed.match(/^[-*]\s+(.*)$/);
    if (ulMatch) {
      flushParagraph(`p-${i}`);
      if (!currentList || currentList.type !== 'ul') {
        flushList(`list-${i}`);
        currentList = { type: 'ul', items: [] };
      }
      currentList.items.push(ulMatch[1]);
      continue;
    }

    // Ordered list items: `1. item`
    const olMatch = trimmed.match(/^\d+\.\s+(.*)$/);
    if (olMatch) {
      flushParagraph(`p-${i}`);
      if (!currentList || currentList.type !== 'ol') {
        flushList(`list-${i}`);
        currentList = { type: 'ol', items: [] };
      }
      currentList.items.push(olMatch[1]);
      continue;
    }

    // Regular text line inside paragraph
    flushList(`list-${i}`);
    paragraphLines.push(trimmed);
  }

  flushParagraph('p-final');
  flushList('list-final');

  return (
    <div
      className={`wr-formatted-markdown ${className}`}
      style={{
        color: 'inherit',
        lineHeight: 1.6,
      }}
    >
      {elements}
    </div>
  );
}

// Inline renderer for bold, italic, inline code, and links
function renderInline(text: string): React.ReactNode[] {
  const parts: React.ReactNode[] = [];
  // Tokenize inline syntax: code `...`, bold **...**, link [...](...)
  const regex = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\))/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = regex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(text.slice(lastIndex, match.index));
    }
    const token = match[0];
    if (token.startsWith('`') && token.endsWith('`')) {
      parts.push(
        <code
          key={match.index}
          style={{
            fontFamily: 'monospace',
            fontSize: '0.88em',
            padding: '2px 5px',
            borderRadius: '4px',
            backgroundColor: 'rgba(20, 20, 25, 0.08)',
            border: '1px solid rgba(0, 0, 0, 0.08)',
            wordBreak: 'break-all',
          }}
        >
          {token.slice(1, -1)}
        </code>
      );
    } else if (token.startsWith('**') && token.endsWith('**')) {
      parts.push(<strong key={match.index}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith('[') && token.includes('](')) {
      const closeBracket = token.indexOf('](');
      const label = token.slice(1, closeBracket);
      const url = token.slice(closeBracket + 2, -1);
      parts.push(
        <a
          key={match.index}
          href={url}
          target="_blank"
          rel="noopener noreferrer"
          style={{ textDecoration: 'underline', color: 'var(--canvas-copper, #c05621)' }}
        >
          {label}
        </a>
      );
    }
    lastIndex = regex.lastIndex;
  }

  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex));
  }

  return parts;
}
