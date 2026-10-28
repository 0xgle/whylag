/** WhyLag mark: a signal trace with a jitter spike and a probe dot. */
export function Logo({ size = 30 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden>
      <rect width="64" height="64" rx="14" fill="var(--accent)" />
      <path
        d="M12 40c5-14 9-14 12 0s7 14 12 0 8-18 12-6"
        fill="none"
        stroke="var(--accent-ink)"
        strokeWidth="5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="50" cy="20" r="4.5" fill="#9fe7c4" />
    </svg>
  );
}
