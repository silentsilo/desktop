type BrandLogoProps = {
  showWordmark?: boolean;
  size?: number;
  className?: string;
};

export function BrandLogo({
  showWordmark = true,
  size = 36,
  className = "",
}: BrandLogoProps) {
  return (
    <span className={`brand-logo ${className}`.trim()}>
      <img src="/icon.svg" alt="" width={size} height={size} aria-hidden />
      {showWordmark && <span className="brand-logo-text">SilentSilo</span>}
    </span>
  );
}
