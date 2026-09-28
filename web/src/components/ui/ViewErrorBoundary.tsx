import { Component, ErrorInfo, ReactNode } from 'react';
import { Button } from './Button';
export class ViewErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Workspace view failed', error, info.componentStack);
  }
  render() {
    if (this.state.failed)
      return (
        <div className="screen" role="alert">
          <h1>This view couldn’t be displayed</h1>
          <p>
            Your preview decisions are still in this session. Retry this view or choose another
            section.
          </p>
          <div>
            <Button onClick={() => this.setState({ failed: false })}>Retry view</Button>
          </div>
        </div>
      );
    return this.props.children;
  }
}
