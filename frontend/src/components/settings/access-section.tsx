import { useTranslation } from "react-i18next";

import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { KeyRound, Mail, ServerCog, ShieldCheck } from "lucide-react";
import type { SystemSettings } from "@/lib/api";
import { SettingsGroup } from "./settings-category-panel";

interface AccessSectionProps {
  settings: SystemSettings;
  onChange: (updates: Partial<SystemSettings>) => void;
}

/**
 * Merged access-control drawer: registration policy plus session/API-key
 * limits, laid out as two side-by-side subgroups on `lg`.
 */
export function AccessSection({ settings, onChange }: AccessSectionProps) {
  const { t } = useTranslation();

  return (
    <div className="grid gap-8 lg:grid-cols-2 lg:gap-12">
      <SettingsGroup label={t("settings.registration")}>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor="registration_enabled">
              {t("settings.allowRegistration")}
            </FieldLabel>
            <FieldDescription>
              {t("settings.allowRegistrationDescription")}
            </FieldDescription>
          </FieldContent>
          <Switch
            id="registration_enabled"
            checked={settings.registration_enabled}
            onCheckedChange={(checked) => onChange({ registration_enabled: checked })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="default_role">{t("settings.defaultUserRole")}</FieldLabel>
          <Input
            id="default_role"
            value={settings.default_user_role}
            onChange={(e) => onChange({ default_user_role: e.target.value })}
          />
          <FieldDescription>{t("settings.defaultUserRoleDescription")}</FieldDescription>
        </Field>
        <Field>
          <FieldLabel className="flex items-center gap-2">
            <Mail className="size-4 text-muted-foreground" aria-hidden="true" />
            {t("settings.emailRegistration")}
          </FieldLabel>
          <FieldDescription>{t("settings.emailRegistrationDescription")}</FieldDescription>
          <div className="overflow-hidden rounded-lg border bg-muted/20">
            <div className="flex flex-wrap items-start justify-between gap-3 border-b px-4 py-3">
              <div className="flex min-w-0 items-start gap-3">
                <div className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground">
                  <ServerCog className="size-4" aria-hidden="true" />
                </div>
                <div className="min-w-0 space-y-1">
                  <p className="text-sm font-medium">{t("settings.emailSmtpTitle")}</p>
                  <p className="text-xs leading-5 text-muted-foreground">
                    {t("settings.emailSmtpDescription")}
                  </p>
                </div>
              </div>
              <Badge variant="secondary" className="gap-1.5">
                <ServerCog className="size-3.5" aria-hidden="true" />
                {t("settings.emailSmtpManaged")}
              </Badge>
            </div>
            <div className="space-y-3 px-4 py-3">
              <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                <KeyRound className="size-3.5" aria-hidden="true" />
                {t("settings.emailSmtpVariables")}
              </div>
              <div className="flex flex-wrap gap-2" aria-label={t("settings.emailSmtpVariables")}>
                {[
                  "MONOIZE_SMTP_HOST",
                  "MONOIZE_SMTP_PORT",
                  "MONOIZE_SMTP_USERNAME",
                  "MONOIZE_SMTP_PASSWORD",
                  "MONOIZE_SMTP_FROM",
                  "MONOIZE_SMTP_FROM_NAME",
                ].map((variable) => (
                  <code key={variable} className="rounded border bg-background px-2 py-1 text-[11px] text-foreground/80">
                    {variable}
                  </code>
                ))}
              </div>
              <p className="text-xs leading-5 text-muted-foreground">
                {t("settings.emailRegistrationSmtpHint")}
              </p>
              <div className="grid gap-3 pt-1 sm:grid-cols-2">
                <Input value={settings.smtp_host} placeholder="smtp.example.com" onChange={(event) => onChange({ smtp_host: event.target.value })} />
                <Input type="number" min="1" max="65535" value={settings.smtp_port} onChange={(event) => onChange({ smtp_port: Number(event.target.value) || 587 })} />
                <Input value={settings.smtp_username} placeholder={t("settings.smtpUsername")} onChange={(event) => onChange({ smtp_username: event.target.value })} />
                <Input type="password" autoComplete="new-password" placeholder={settings.smtp_password === "__set__" ? t("settings.smtpPasswordConfigured") : t("settings.smtpPassword")} onChange={(event) => onChange({ smtp_password: event.target.value })} />
                <Input type="email" value={settings.smtp_from_email} placeholder={t("settings.smtpFromEmail")} onChange={(event) => onChange({ smtp_from_email: event.target.value })} />
                <Input value={settings.smtp_from_name} placeholder={t("settings.smtpFromName")} onChange={(event) => onChange({ smtp_from_name: event.target.value })} />
              </div>
            </div>
            <div className="grid gap-px border-t bg-border sm:grid-cols-3">
              {[
                ["settings.emailVerificationExpiry", "15 min"],
                ["settings.emailVerificationResend", "60 s"],
                ["settings.emailVerificationAttempts", "5"],
              ].map(([label, value]) => (
                <div key={label} className="flex items-center gap-2 bg-muted/20 px-4 py-3 text-xs">
                  <ShieldCheck className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                  <span className="text-muted-foreground">{t(label)}</span>
                  <span className="ml-auto font-medium text-foreground">{value}</span>
                </div>
              ))}
            </div>
          </div>
        </Field>
      </SettingsGroup>

      <SettingsGroup label={t("settings.sessionSecurity")}>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor="captcha_enabled">
              {t("settings.captchaEnabled")}
            </FieldLabel>
            <FieldDescription>
              {t("settings.captchaEnabledDescription")}
            </FieldDescription>
          </FieldContent>
          <Switch
            id="captcha_enabled"
            checked={settings.captcha_enabled}
            onCheckedChange={(checked) => onChange({ captcha_enabled: checked })}
          />
        </Field>
        <div className="grid gap-6 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="session_ttl">{t("settings.sessionDuration")}</FieldLabel>
            <Input
              id="session_ttl"
              type="number"
              min="1"
              value={settings.session_ttl_days}
              onChange={(e) =>
                onChange({ session_ttl_days: parseInt(e.target.value) || 7 })
              }
            />
            <FieldDescription>{t("settings.sessionDurationDescription")}</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="max_api_keys">{t("settings.maxApiKeys")}</FieldLabel>
            <Input
              id="max_api_keys"
              type="number"
              min="1"
              value={settings.api_key_max_per_user}
              onChange={(e) =>
                onChange({ api_key_max_per_user: parseInt(e.target.value) || 10 })
              }
            />
            <FieldDescription>{t("settings.maxApiKeysDescription")}</FieldDescription>
          </Field>
        </div>
      </SettingsGroup>
    </div>
  );
}
