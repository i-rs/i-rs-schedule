import { useToasts, dismissToast, type ToastVariant } from "@/hooks/useToast";
import { CheckCircle2, AlertCircle, Info, X } from "lucide-react";
import { cn } from "@/lib/utils";

const variantStyles: Record<ToastVariant, { icon: typeof Info; ring: string; iconColor: string }> = {
  success: { icon: CheckCircle2, ring: "border-l-emerald-500", iconColor: "text-emerald-500" },
  error: { icon: AlertCircle, ring: "border-l-destructive", iconColor: "text-destructive" },
  default: { icon: Info, ring: "border-l-muted-foreground", iconColor: "text-muted-foreground" },
};

export function ToastContainer() {
  const toasts = useToasts();

  return (
    <div className="fixed top-4 right-4 z-[100] flex flex-col gap-2 w-[calc(100vw-2rem)] sm:w-80">
      {toasts.map((t) => {
        const cfg = variantStyles[t.variant];
        const Icon = cfg.icon;
        return (
          <div
            key={t.id}
            className={cn(
              "flex items-start gap-2.5 rounded-lg border border-l-4 bg-card p-3 shadow-md",
              "animate-in fade-in slide-in-from-top-2 duration-200",
              cfg.ring,
            )}
          >
            <Icon className={cn("h-4 w-4 mt-0.5 shrink-0", cfg.iconColor)} />
            <p className="text-sm flex-1 leading-relaxed">{t.message}</p>
            <button
              onClick={() => dismissToast(t.id)}
              className="text-muted-foreground hover:text-foreground shrink-0"
              aria-label="Dismiss"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
