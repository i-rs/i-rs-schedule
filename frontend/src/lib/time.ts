/** 相对时间格式化:just now / 5m ago / 3h ago / 2d ago。 */
export function timeAgo(iso: string): string {
  const seconds = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

/** 完整本地时间,用于 title 提示。 */
export function fullTime(iso: string): string {
  return new Date(iso).toLocaleString();
}
