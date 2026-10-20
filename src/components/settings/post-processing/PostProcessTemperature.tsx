import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";

/** Where the slider starts when no temperature has been chosen yet. */
const DEFAULT_TEMPERATURE = 0;

/**
 * Temperature for post-processing requests. `null` means "send none" and
 * leaves the model's own default in charge.
 */
export const PostProcessTemperature: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const temperature = getSetting("post_process_temperature");
  const useModelDefault = temperature === null;
  // Kept while the model's default is in use, so turning that off again
  // brings back the value the user had instead of resetting it.
  const [lastTemperature, setLastTemperature] = useState(
    temperature ?? DEFAULT_TEMPERATURE,
  );

  const handleUseModelDefault = (checked: boolean) => {
    if (checked) {
      if (typeof temperature === "number") setLastTemperature(temperature);
      updateSetting("post_process_temperature", null);
    } else {
      updateSetting("post_process_temperature", lastTemperature);
    }
  };

  return (
    <>
      <Slider
        value={temperature ?? lastTemperature}
        onChange={(value) => updateSetting("post_process_temperature", value)}
        min={0}
        max={1}
        step={0.1}
        disabled={useModelDefault}
        label={t("settings.postProcessing.api.temperature.title")}
        description={t("settings.postProcessing.api.temperature.description")}
        descriptionMode="tooltip"
        grouped
        formatValue={(value) => value.toFixed(1)}
      />
      <ToggleSwitch
        checked={useModelDefault}
        onChange={handleUseModelDefault}
        isUpdating={isUpdating("post_process_temperature")}
        label={t(
          "settings.postProcessing.api.temperature.useModelDefault.title",
        )}
        description={t(
          "settings.postProcessing.api.temperature.useModelDefault.description",
        )}
        descriptionMode="tooltip"
        grouped
      />
    </>
  );
};
