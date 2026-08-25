import React, { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { commands } from "@/bindings";
import { SettingContainer, SettingsGroup, ToggleSwitch } from "@/components/ui";
import { Alert } from "@/components/ui/Alert";
import { useSettings } from "@/hooks/useSettings";

import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import type { ModelOption } from "../PostProcessingSettingsApi/types";

const DEFAULT_BASE_URL = "https://api.groq.com/openai/v1";
const DEFAULT_MODEL = "whisper-large-v3-turbo";

/// An endpoint's model list mixes speech models in with everything else it
/// serves. Narrow to the ones that can transcribe, but never to nothing: a
/// custom endpoint is free to name its models anything at all.
const SPEECH_MODEL_PATTERN = /whisper|distil|transcrib|asr|voxtral|parakeet/i;

const narrowToSpeechModels = (models: string[]): string[] => {
  const speech = models.filter((model) => SPEECH_MODEL_PATTERN.test(model));
  return speech.length > 0 ? speech : models;
};

export const TranscriptionApiSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();

  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);

  const enabled = settings?.transcription_api_enabled ?? false;
  const baseUrl = settings?.transcription_api_base_url ?? DEFAULT_BASE_URL;
  const apiKey = settings?.transcription_api_key ?? "";
  const model = settings?.transcription_api_model ?? DEFAULT_MODEL;
  const localFallback = settings?.transcription_api_local_fallback ?? false;

  const options = useMemo<ModelOption[]>(
    () => modelOptions.map((value) => ({ value, label: value })),
    [modelOptions],
  );

  const refreshModels = useCallback(async () => {
    setIsFetchingModels(true);
    setModelError(null);
    try {
      const result = await commands.listTranscriptionApiModels(baseUrl, apiKey);
      if (result.status === "ok") {
        setModelOptions(narrowToSpeechModels(result.data));
      } else {
        setModelOptions([]);
        setModelError(result.error);
      }
    } catch (error) {
      setModelOptions([]);
      setModelError(String(error));
    } finally {
      setIsFetchingModels(false);
    }
  }, [baseUrl, apiKey]);

  return (
    <SettingsGroup title={t("settings.transcriptionApi.groupTitle")}>
      <ToggleSwitch
        checked={enabled}
        onChange={(checked) =>
          updateSetting("transcription_api_enabled", checked)
        }
        isUpdating={isUpdating("transcription_api_enabled")}
        label={t("settings.transcriptionApi.enable.title")}
        description={t("settings.transcriptionApi.enable.description")}
        grouped
      />

      {enabled && (
        <>
          <Alert variant="warning" contained>
            {t("settings.transcriptionApi.privacyWarning")}
          </Alert>

          <SettingContainer
            title={t("settings.transcriptionApi.baseUrl.title")}
            description={t("settings.transcriptionApi.baseUrl.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped
          >
            <div className="flex items-center gap-2">
              <BaseUrlField
                value={baseUrl}
                onBlur={(value) =>
                  updateSetting(
                    "transcription_api_base_url",
                    value.trim() || DEFAULT_BASE_URL,
                  )
                }
                placeholder={DEFAULT_BASE_URL}
                disabled={isUpdating("transcription_api_base_url")}
              />
            </div>
          </SettingContainer>

          <SettingContainer
            title={t("settings.transcriptionApi.apiKey.title")}
            description={t("settings.transcriptionApi.apiKey.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped
          >
            <div className="flex items-center gap-2">
              <ApiKeyField
                value={apiKey}
                onBlur={(value) =>
                  updateSetting("transcription_api_key", value.trim())
                }
                placeholder={t("settings.transcriptionApi.apiKey.placeholder")}
                disabled={isUpdating("transcription_api_key")}
              />
            </div>
          </SettingContainer>

          <SettingContainer
            title={t("settings.transcriptionApi.model.title")}
            description={t("settings.transcriptionApi.model.description")}
            descriptionMode="tooltip"
            layout="stacked"
            grouped
          >
            <div className="flex items-center gap-2">
              <ModelSelect
                value={model}
                options={options}
                isLoading={isFetchingModels}
                disabled={isUpdating("transcription_api_model")}
                placeholder={t("settings.transcriptionApi.model.placeholder")}
                onSelect={(value) =>
                  updateSetting("transcription_api_model", value)
                }
                onCreate={(value) =>
                  updateSetting("transcription_api_model", value)
                }
                onBlur={() => {}}
              />
              <button
                type="button"
                onClick={refreshModels}
                disabled={isFetchingModels}
                className="text-xs text-mid-gray hover:text-text underline disabled:opacity-50"
              >
                {t("settings.transcriptionApi.model.refresh")}
              </button>
            </div>
          </SettingContainer>

          {modelError && (
            <Alert variant="error" contained>
              {modelError}
            </Alert>
          )}

          <ToggleSwitch
            checked={localFallback}
            onChange={(checked) =>
              updateSetting("transcription_api_local_fallback", checked)
            }
            isUpdating={isUpdating("transcription_api_local_fallback")}
            label={t("settings.transcriptionApi.localFallback.title")}
            description={t(
              "settings.transcriptionApi.localFallback.description",
            )}
            grouped
          />
        </>
      )}
    </SettingsGroup>
  );
};
