import { Component, Fragment, type ErrorInfo, type ReactNode } from "react";

type Props = { id: string; children: ReactNode };
type State = { error: boolean; revision: number; incident: string | null };

/** Keep one broken widget from taking the command center with it. */
export class WidgetBoundary extends Component<Props, State> {
  state: State = { error: false, revision: 0, incident: null };

  static getDerivedStateFromError(): State {
    return { error: true, revision: 0, incident: `widget-${Date.now().toString(36)}` };
  }

  componentDidCatch(error: Error, _info: ErrorInfo) {
    // Diagnostics stay in the browser console; the operator surface must not
    // render private paths, bearer values, or an arbitrary thrown string.
    console.error(`widget ${this.props.id} failed`, error);
  }

  render() {
    if (!this.state.error) return <Fragment key={this.state.revision}>{this.props.children}</Fragment>;
    return (
      <div className="widget-failure" role="alert">
        <b>{this.props.id} is unavailable</b>
        <p className="dim small">This widget failed to render. The rest of farseer is still live.</p>
        <p className="dim small mono">incident {this.state.incident}</p>
        <button
          className="chip"
          onClick={() => this.setState((current) => ({ error: false, revision: current.revision + 1, incident: null }))}
        >
          retry
        </button>
      </div>
    );
  }
}
