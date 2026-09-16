import { Component, type ReactNode } from "react";
import { AlertTriangle, RotateCw } from "lucide-react";
import { t } from "@/lib/i18n";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/** 捕获子树渲染异常,显示友好错误页而非白屏。 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error) {
    console.error("Unhandled render error:", error);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-center justify-center py-24 gap-4 text-muted-foreground">
          <span className="flex h-12 w-12 items-center justify-center rounded-full bg-destructive/10">
            <AlertTriangle className="h-6 w-6 text-destructive" />
          </span>
          <div className="text-center">
            <p className="text-sm font-medium text-foreground">{t("Something went wrong")}</p>
            <p className="text-xs mt-1 max-w-sm break-all text-muted-foreground/70">
              {this.state.error.message}
            </p>
          </div>
          <button
            onClick={() => this.setState({ error: null })}
            className="inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm transition-colors hover:bg-muted cursor-pointer"
          >
            <RotateCw className="h-3.5 w-3.5" /> {t("Try again")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
