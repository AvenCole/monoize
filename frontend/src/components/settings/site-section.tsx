import { useTranslation } from "react-i18next";

import {
  Field,
  FieldDescription,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ImagePlus, Loader2, Trash2, Upload } from "lucide-react";
import { MonoizeLogo } from "@/components/MonoizeLogo";
import type { SystemSettings } from "@/lib/api";
import { api } from "@/lib/api";
import { useRef, useState } from "react";

interface SiteSectionProps {
  settings: SystemSettings;
  onChange: (updates: Partial<SystemSettings>) => void;
}

/** Site identity fields: name, description, downstream API base URL. */
export function SiteSection({ settings, onChange }: SiteSectionProps) {
  const { t } = useTranslation();
  const [uploading, setUploading] = useState(false);
  const [logoError, setLogoError] = useState("");
  const [logoConfigured, setLogoConfigured] = useState(true);
  const [logoVersion, setLogoVersion] = useState(0);
  const fileInputRef = useRef<HTMLInputElement>(null);

  return (
    <div className="grid gap-6 sm:grid-cols-2">
      <Field>
        <FieldLabel htmlFor="site_name">{t("settings.siteName")}</FieldLabel>
        <Input
          id="site_name"
          value={settings.site_name}
          onChange={(e) => onChange({ site_name: e.target.value })}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="site_description">{t("settings.siteDescription")}</FieldLabel>
        <Input
          id="site_description"
          value={settings.site_description}
          onChange={(e) => onChange({ site_description: e.target.value })}
        />
      </Field>
      <Field className="sm:col-span-2">
        <FieldLabel htmlFor="site_logo">{t("settings.siteLogo")}</FieldLabel>
        <div className="overflow-hidden rounded-lg border bg-muted/20">
          <div className="flex flex-wrap items-center gap-4 px-4 py-4">
            <div className="flex size-16 shrink-0 items-center justify-center rounded-md border bg-background p-2">
              {logoConfigured ? (
                <img
                  key={logoVersion}
                  src={`/api/dashboard/branding/logo?v=${logoVersion}`}
                  alt={settings.site_name || t("settings.logoPreview")}
                  className="size-full object-contain"
                  onError={() => setLogoConfigured(false)}
                />
              ) : (
                <MonoizeLogo className="size-10 text-foreground/80" aria-label={t("settings.logoPreview")} />
              )}
            </div>
            <div className="min-w-0 flex-1 space-y-1">
              <div className="flex flex-wrap items-center gap-2">
                <p className="text-sm font-medium">
                  {logoConfigured ? t("settings.logoConfigured") : t("settings.logoNotConfigured")}
                </p>
                <Badge variant="secondary" className="gap-1.5 text-[11px]">
                  <ImagePlus className="size-3" aria-hidden="true" />
                  PNG / JPEG / WebP
                </Badge>
              </div>
              <p className="text-xs leading-5 text-muted-foreground">{t("settings.logoFormatHint")}</p>
            </div>
            <div className="flex shrink-0 flex-wrap items-center gap-2">
              <input
                ref={fileInputRef}
                id="site_logo"
                type="file"
                accept="image/png,image/jpeg,image/webp"
                className="sr-only"
                disabled={uploading}
                onChange={async (e) => {
                  const file = e.target.files?.[0];
                  if (!file) return;
                  if (file.size > 1024 * 1024) {
                    setLogoError(t("settings.logoTooLarge"));
                    e.target.value = "";
                    return;
                  }
                  setUploading(true);
                  setLogoError("");
                  try {
                    await api.uploadLogo(file);
                    setLogoConfigured(true);
                    setLogoVersion((version) => version + 1);
                  } catch (err) {
                    setLogoError(err instanceof Error ? err.message : t("settings.logoUploadFailed"));
                  } finally {
                    setUploading(false);
                    e.target.value = "";
                  }
                }}
              />
              <Button type="button" size="sm" disabled={uploading} onClick={() => fileInputRef.current?.click()}>
                {uploading ? <Loader2 className="size-4 animate-spin" aria-hidden="true" /> : <Upload className="size-4" aria-hidden="true" />}
                {t("settings.logoUpload")}
              </Button>
              <Button type="button" size="sm" variant="outline" disabled={uploading || !logoConfigured} onClick={async () => {
                setUploading(true);
                setLogoError("");
                try {
                  await api.deleteLogo();
                  setLogoConfigured(false);
                } catch (err) {
                  setLogoError(err instanceof Error ? err.message : t("settings.logoUploadFailed"));
                } finally {
                  setUploading(false);
                }
              }}>
                <Trash2 className="size-4" aria-hidden="true" />
                {t("settings.removeLogo")}
              </Button>
            </div>
          </div>
          {logoError && <div className="border-t px-4 py-2 text-xs text-destructive">{logoError}</div>}
        </div>
      </Field>
      <Field className="sm:col-span-2">
        <FieldLabel htmlFor="api_base_url">{t("settings.apiBaseUrl")}</FieldLabel>
        <Input
          id="api_base_url"
          value={settings.api_base_url}
          onChange={(e) => onChange({ api_base_url: e.target.value })}
          placeholder={t("settings.apiBaseUrlPlaceholder")}
        />
        <FieldDescription>{t("settings.apiBaseUrlDescription")}</FieldDescription>
      </Field>
      <Field className="sm:col-span-2">
        <FieldLabel htmlFor="recharge_public_origin">
          {t("settings.rechargePublicOrigin")}
        </FieldLabel>
        <Input
          id="recharge_public_origin"
          value={settings.recharge_public_origin}
          onChange={(e) => onChange({ recharge_public_origin: e.target.value })}
          placeholder={t("settings.rechargePublicOriginPlaceholder")}
        />
        <FieldDescription>
          {t("settings.rechargePublicOriginDescription")}
        </FieldDescription>
      </Field>
    </div>
  );
}
