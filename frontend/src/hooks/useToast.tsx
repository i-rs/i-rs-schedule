import { useEffect, useState, useSyncExternalStore } from "react";

export type ToastVariant = "default" | "success" | "error";

export interface ToastItem {
  id: number;
  message: string;
  variant: ToastVariant;
}

type Listener = () => void;

// 极简全局 toast store:模块级单例,组件通过 useSyncExternalStore 订阅。
// 不引入额外依赖,API 为 toast.success/error/message。
let toasts: ToastItem[] = [];
let listeners: Listener[] = [];
let nextId = 1;

function emit() {
  for (const l of listeners) l();
}

function subscribe(listener: Listener) {
  listeners.push(listener);
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
}

function getSnapshot() {
  return toasts;
}

function remove(id: number) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

function push(message: string, variant: ToastVariant) {
  const id = nextId++;
  toasts = [...toasts, { id, message, variant }];
  emit();
  setTimeout(() => remove(id), 3500);
}

export const toast = {
  success: (message: string) => push(message, "success"),
  error: (message: string) => push(message, "error"),
  message: (message: string) => push(message, "default"),
};

/** 订阅全局 toast 列表。 */
export function useToasts(): ToastItem[] {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** 供 ToastContainer 使用:移除某条 toast。 */
export function dismissToast(id: number) {
  remove(id);
}

/** hook 形式:为单条 toast 提供自动消失计时(供组件内部用)。 */
export function useAutoDismiss(id: number, ms = 3500) {
  const [visible, setVisible] = useState(true);
  useEffect(() => {
    const timer = setTimeout(() => setVisible(false), ms);
    return () => clearTimeout(timer);
  }, [id, ms]);
  return visible;
}
