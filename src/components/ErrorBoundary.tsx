import { Component, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
  /** Chamado ao clicar em "Tentar de novo", antes de tentar renderizar os filhos outra vez. */
  onReset?: () => void;
}

interface ErrorBoundaryState {
  failed: boolean;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  private reset = () => {
    this.props.onReset?.();
    this.setState({ failed: false });
  };

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="error" role="alert">
        <p>Não foi possível carregar</p>
        <button type="button" className="btn" onClick={this.reset}>
          Tentar de novo
        </button>
      </div>
    );
  }
}
