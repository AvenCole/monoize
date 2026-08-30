import { useTranslation } from "react-i18next";

import {
  Field,
  FieldDescription,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import type { SystemSettings } from "@/lib/api";
import { api } from "@/lib/api";
import { useState } from "react";

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
        <div className="flex items-center gap-3">
          {logoConfigured ? <img src={`/api/dashboard/branding/logo?v=${logoVersion}`} alt={settings.site_name || "Monoize"} className="size-12 rounded-md border object-contain" onError={() => setLogoConfigured(false)} /> : <div className="flex size-12 items-center justify-center rounded-md border text-xs text-muted-foreground">Monoize</div>}
          <Input id="site_logo" type="file" accept="image/png,image/jpeg,image/webp" disabled={uploading} onChange={async (e) => { const file = e.target.files?.[0]; if (!file) return; if (file.size > 1024 * 1024) { setLogoError(t("settings.logoTooLarge")); return; } setUploading(true); setLogoError(""); try { await api.uploadLogo(file); setLogoConfigured(true); setLogoVersion((version) => version + 1); e.target.value = ""; } catch (err) { setLogoError(err instanceof Error ? err.message : t("settings.logoUploadFailed")); } finally { setUploading(false); } }} />
          <Button type="button" variant="ghost" className="text-destructive" disabled={uploading} onClick={async () => { setUploading(true); setLogoError(""); try { await api.deleteLogo(); setLogoConfigured(false); } catch (err) { setLogoError(err instanceof Error ? err.message : t("settings.logoUploadFailed")); } finally { setUploading(false); } }}>{t("settings.removeLogo")}</Button>
        </div>
        {logoError && <FieldDescription className="text-destructive">{logoError}</FieldDescription>}
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
