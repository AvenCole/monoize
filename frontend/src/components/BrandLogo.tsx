import { useState } from "react";
import { MonoizeLogo } from "@/components/MonoizeLogo";

const logoUrl = "/api/dashboard/branding/logo";

interface BrandLogoProps {
  className?: string;
  alt?: string;
}

/** Render the uploaded logo when available and fall back to the built-in mark. */
export function BrandLogo({ className, alt = "" }: BrandLogoProps) {
  const [failed, setFailed] = useState(false);
  if (failed) return <MonoizeLogo className={className} aria-label={alt} />;
  return <img src={logoUrl} alt={alt} className={className} onError={() => setFailed(true)} />;
}
