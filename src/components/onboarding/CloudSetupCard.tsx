import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Cloud } from "lucide-react";

import { commands } from "@/bindings";

const DEFAULT_BASE_URL = "https://api.groq.com/openai/v1";
const DEFAULT_MODEL = "whisper-large-v3-turbo";

/// An endpoint's model list mixes speech models in with everything else it
/// serves. Narrow to the ones that can transcribe, but never to nothing.
const SPEECH_MODEL_PATTERN = /whisper|distil|transcrib|asr|voxtral|parakeet/i;

interface CloudSetupCardProps {
  /// Advance the onboarding flow. The backend has already marked onboarding
  /// complete by the time this runs.
  onDone: () => void;
  disabled?: boolean;
}

/// Lets a first-run user finish onboarding without downloading a local model,
/// which is the whole point of the Transcription API on a low-end machine.
/// Lives in its own file so `Onboarding.tsx` only gains a render line.
export const CloudSetupCard: React.FC<CloudSetupCardProps> = ({
  onDone,
  disabled = false,
}) => {
  const { t } = useTranslation();

  const [expanded, setExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState(DEFAULT_BASE_URL);
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadModels = async () => {
    setIsBusy(true);
    setError(null);
    try {
      const result = await commands.listTranscriptionApiModels(baseUrl, apiKey);
      if (result.status === "ok") {
        const speech = result.data.filter((id) =>
          SPEECH_MODEL_PATTERN.test(id),
        );
        const options = speech.length > 0 ? speech : result.data;
        setModelOptions(options);
        if (options.length > 0 && !options.includes(model)) {
          setModel(options[0]);
        }
      } else {
        setError(result.error);
      }
    } catch (caught) {
      setError(String(caught));
    } finally {
      setIsBusy(false);
    }
  };

  const start = async () => {
    setIsBusy(true);
    setError(null);
    try {
      const result = await commands.completeOnboardingWithTranscriptionApi(
        baseUrl,
        apiKey,
        model,
      );
      if (result.status === "ok") {
        onDone();
      } else {
        setError(result.error);
      }
    } catch (caught) {
      setError(String(caught));
    } finally {
      setIsBusy(false);
    }
  };

  if (!expanded) {
    return (
      <div className="rounded-lg border border-mid-gray/30 p-4 text-left">
        <div className="flex items-start gap-3">
          <Cloud className="w-5 h-5 mt-0.5 shrink-0 text-text/60" />
          <div className="flex-1">
            <h2 className="text-sm font-medium">
              {t("onboarding.transcriptionApi.title")}
            </h2>
            <p className="text-xs text-text/60 mt-1">
              {t("onboarding.transcriptionApi.description")}
            </p>
          </div>
          <button
            type="button"
            disabled={disabled}
            onClick={() => setExpanded(true)}
            className="text-xs px-3 py-1.5 rounded-md border border-mid-gray/40 hover:bg-mid-gray/10 disabled:opacity-50 shrink-0"
          >
            {t("onboarding.transcriptionApi.setUp")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-mid-gray/30 p-4 text-left space-y-3">
      <div className="flex items-start gap-3">
        <Cloud className="w-5 h-5 mt-0.5 shrink-0 text-text/60" />
        <div className="flex-1">
          <h2 className="text-sm font-medium">
            {t("onboarding.transcriptionApi.title")}
          </h2>
          <p className="text-xs text-text/60 mt-1">
            {t("onboarding.transcriptionApi.privacyWarning")}
          </p>
        </div>
      </div>

      <label className="block text-xs text-text/70">
        {t("onboarding.transcriptionApi.baseUrl")}
        <input
          type="text"
          value={baseUrl}
          onChange={(event) => setBaseUrl(event.target.value)}
          className="mt-1 w-full px-2 py-1.5 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </label>

      <label className="block text-xs text-text/70">
        {t("onboarding.transcriptionApi.apiKey")}
        <input
          type="password"
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
          placeholder={t("onboarding.transcriptionApi.apiKeyPlaceholder")}
          className="mt-1 w-full px-2 py-1.5 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </label>

      <label className="block text-xs text-text/70">
        {t("onboarding.transcriptionApi.model")}
        {modelOptions.length > 0 ? (
          <select
            value={model}
            onChange={(event) => setModel(event.target.value)}
            className="mt-1 w-full px-2 py-1.5 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
          >
            {modelOptions.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        ) : (
          <input
            type="text"
            value={model}
            onChange={(event) => setModel(event.target.value)}
            className="mt-1 w-full px-2 py-1.5 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
          />
        )}
      </label>

      {error && <p className="text-xs text-red-500">{error}</p>}

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={start}
          disabled={isBusy || disabled}
          className="text-xs px-3 py-1.5 rounded-md bg-logo-primary text-white disabled:opacity-50"
        >
          {isBusy
            ? t("onboarding.transcriptionApi.checking")
            : t("onboarding.transcriptionApi.start")}
        </button>
        <button
          type="button"
          onClick={loadModels}
          disabled={isBusy || disabled}
          className="text-xs px-3 py-1.5 rounded-md border border-mid-gray/40 hover:bg-mid-gray/10 disabled:opacity-50"
        >
          {t("onboarding.transcriptionApi.loadModels")}
        </button>
        <button
          type="button"
          onClick={() => setExpanded(false)}
          disabled={isBusy}
          className="text-xs text-text/60 hover:text-text underline disabled:opacity-50"
        >
          {t("onboarding.transcriptionApi.cancel")}
        </button>
      </div>
    </div>
  );
};

export default CloudSetupCard;
