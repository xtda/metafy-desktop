<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { onMount, tick } from "svelte";

  type View = "record" | "library" | "search" | "settings";
  type NativeStatus = "idle" | "loading" | "ready" | "error";
  type CaptureUiState = "idle" | "recording" | "processing";
  type RecordingSessionStatus = "capturing" | "stopped" | "failed";
  type RecordingStatus =
    | "pending"
    | "capturing"
    | "processing"
    | "completed"
    | "failed";
  type TranscriptStatus =
    | "not_started"
    | "processing"
    | "completed"
    | "failed";
  type AiStatus = "disabled" | "pending" | "processing" | "completed" | "failed";
  type JobState = "queued" | "running" | "succeeded" | "failed" | "cancelled";
  type CapturePermissionState =
    | "granted"
    | "prompt_required"
    | "denied"
    | "unavailable"
    | "unknown";

  type AppBootstrap = {
    appName: string;
    runtime: {
      shell: string;
      native: string;
      frontend: string;
      packageManager: string;
    };
    localOnly: {
      coreNetworkRequired: boolean;
      rawMediaLeavesDevice: boolean;
      storage: StoragePathsSnapshot;
      optionalAi: {
        enabled: boolean;
        payloadScope: string;
      };
    };
    storage: StorageOverview;
    nativeBoundaries: Array<{
      domain: string;
      owner: string;
      status: string;
    }>;
    availableCommands: string[];
  };

  type StoragePathsSnapshot = {
    root: string;
    databaseFile: string;
    recordingsDirectory: string;
    whisperModelsDirectory: string;
    tempDirectory: string;
  };

  type StorageOverview = {
    paths: StoragePathsSnapshot;
    schemaVersion: number;
    sqliteInitialized: boolean;
    tables: string[];
    recordingCount: number;
    processingJobCount: number;
  };

  type WhisperBinaryStatus = {
    available: boolean;
    path: string | null;
    envVar: string;
    candidates: string[];
  };

  type WhisperLocalModel = {
    name: string;
    fileName: string;
    path: string;
  };

  type WhisperModelStatus = {
    selectedModel: string;
    defaultModel: string;
    expectedFileName: string;
    modelPath: string;
    modelsDirectory: string;
    exists: boolean;
    availableModels: WhisperLocalModel[];
    binary: WhisperBinaryStatus;
  };

  type Recording = {
    id: string;
    title: string;
    status: RecordingStatus;
    recordingDirectory: string;
    mediaPath: string | null;
    thumbnailPath: string | null;
    durationMs: number | null;
    capturedAt: string | null;
    createdAt: string;
    updatedAt: string;
    completedAt: string | null;
    failureMessage: string | null;
  };

  type RecordingAssetPaths = {
    mediaPath: string | null;
    thumbnailPath: string | null;
  };

  type RecordingSession = {
    id: string;
    recordingId: string;
    status: RecordingSessionStatus;
    tempDirectory: string;
    videoPath: string;
    audioPath: string | null;
    metadataPath: string;
    screenSourceId: string;
    microphoneDeviceId: string | null;
    includeMicrophone: boolean;
    width: number | null;
    height: number | null;
    frameRate: number;
    frameCount: number;
    audioByteCount: number;
    audioSampleRate: number | null;
    audioChannels: number | null;
    audioSampleFormat: string | null;
    startedAt: string;
    stoppedAt: string | null;
    durationMs: number | null;
    failureMessage: string | null;
    createdAt: string;
    updatedAt: string;
  };

  type RecordingSessionEnvelope = {
    recording: Recording;
    session: RecordingSession;
  };

  type Transcript = {
    id: string;
    recordingId: string;
    status: TranscriptStatus;
    language: string | null;
    modelName: string | null;
    rawJsonPath: string | null;
    text: string | null;
    createdAt: string;
    updatedAt: string;
    completedAt: string | null;
    failureMessage: string | null;
  };

  type TranscriptSegment = {
    id: string;
    transcriptId: string;
    recordingId: string;
    segmentIndex: number;
    startMs: number;
    endMs: number;
    text: string;
    confidence: number | null;
  };

  type TranscriptWithSegments = {
    transcript: Transcript;
    segments: TranscriptSegment[];
  };

  type TranscriptSearchResult = {
    recordingId: string;
    recordingTitle: string;
    transcriptId: string;
    segmentId: string;
    segmentIndex: number;
    startMs: number;
    endMs: number;
    text: string;
    snippet: string;
    rank: number;
    mediaPath: string | null;
    thumbnailPath: string | null;
    capturedAt: string | null;
    createdAt: string;
  };

  type TranscriptSearchIndexSummary = {
    indexedSegmentCount: number;
  };

  type AiSummary = {
    id: string;
    recordingId: string;
    status: AiStatus;
    modelName: string | null;
    summaryText: string | null;
    actionItemsJson: string | null;
    decisionsJson: string | null;
    questionsJson: string | null;
    risksJson: string | null;
    chaptersJson: string | null;
    createdAt: string;
    updatedAt: string;
    completedAt: string | null;
    failureMessage: string | null;
  };

  type AiSettings = {
    enabled: boolean;
    provider: string;
    modelName: string;
    endpointUrl: string;
    hasApiKey: boolean;
    updatedAt: string | null;
  };

  type AiOutputItem = {
    primary: string;
    detail: string;
  };

  type ProcessingJob = {
    id: string;
    recordingId: string | null;
    kind: string;
    state: JobState;
    priority: number;
    attempts: number;
    maxAttempts: number;
    inputJson: string | null;
    outputJson: string | null;
    errorMessage: string | null;
    interrupted: boolean;
    lastErrorAt: string | null;
    createdAt: string;
    updatedAt: string;
    startedAt: string | null;
    completedAt: string | null;
  };

  type CaptureCapability = {
    supported: boolean;
    permissionState: CapturePermissionState;
    error: string | null;
  };

  type CaptureDisplaySource = {
    id: string;
    title: string;
    primary: boolean;
  };

  type MicrophoneDevice = {
    id: string;
    name: string;
    isDefault: boolean;
    channels: number | null;
    sampleRate: number | null;
    error: string | null;
  };

  type CaptureSelection = {
    screenSourceId: string | null;
    microphoneDeviceId: string | null;
    includeMicrophone: boolean;
    updatedAt: string | null;
  };

  type ValidatedCaptureConfig = {
    screenSource: CaptureDisplaySource;
    microphone: MicrophoneDevice | null;
    includeMicrophone: boolean;
  };

  type CaptureStatus = {
    screen: CaptureCapability;
    microphone: CaptureCapability;
    displays: CaptureDisplaySource[];
    microphones: MicrophoneDevice[];
    selection: CaptureSelection;
    validatedConfig: ValidatedCaptureConfig | null;
    validationErrors: string[];
  };

  const dateFormatter = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });

  const navItems: Array<{ id: View; label: string; detail: string }> = [
    { id: "record", label: "Record", detail: "Session controls" },
    { id: "library", label: "Library", detail: "Local recordings" },
    { id: "search", label: "Search", detail: "Transcript history" },
    { id: "settings", label: "Settings", detail: "Local defaults" },
  ];

  const statusLabels: Record<RecordingStatus, string> = {
    pending: "Pending",
    capturing: "Recording",
    processing: "Processing",
    completed: "Complete",
    failed: "Failed",
  };

  const sessionStatusLabels: Record<RecordingSessionStatus, string> = {
    capturing: "Capturing",
    stopped: "Stopped",
    failed: "Failed",
  };

  const transcriptStatusLabels: Record<TranscriptStatus, string> = {
    not_started: "Not started",
    processing: "Processing",
    completed: "Complete",
    failed: "Failed",
  };

  const commonWhisperModels = ["tiny.en", "base.en", "small.en", "medium.en"];
  const defaultAiEndpointUrl = "https://api.openai.com/v1/chat/completions";

  const jobStateLabels: Record<JobState, string> = {
    queued: "Queued",
    running: "Running",
    succeeded: "Succeeded",
    failed: "Failed",
    cancelled: "Cancelled",
  };

  const jobKindLabels: Record<string, string> = {
    encode_recording: "Encode recording",
    extract_audio: "Extract audio",
    run_whisper: "Run Whisper",
    transcribe_recording: "Transcribe recording",
    index_transcript: "Index transcript",
    generate_thumbnail: "Generate thumbnail",
    ai_summary: "AI summary",
    clean_temp_files: "Clean temp files",
  };
  const retryableJobKinds = new Set([
    "encode_recording",
    "transcribe_recording",
    "ai_summary",
    "index_transcript",
    "clean_temp_files",
  ]);

  const aiStatusLabels: Record<AiStatus, string> = {
    disabled: "Disabled",
    pending: "Pending",
    processing: "Processing",
    completed: "Complete",
    failed: "Failed",
  };

  const lifecycleStates: Array<{ status: RecordingStatus; description: string }> =
    [
      {
        status: "pending",
        description: "Created before capture writes files.",
      },
      {
        status: "processing",
        description: "Encoding, transcription, or AI job work.",
      },
      {
        status: "failed",
        description: "Failure detail remains attached.",
      },
      {
        status: "completed",
        description: "Ready for playback and transcript review.",
      },
    ];

  let currentView = $state<View>("record");
  let nativeStatus = $state<NativeStatus>("idle");
  let captureUiState = $state<CaptureUiState>("idle");
  let bootstrap = $state<AppBootstrap | null>(null);
  let recordings = $state<Recording[]>([]);
  let selectedRecordingId = $state<string | null>(null);
  let selectedRecordingSession = $state<RecordingSession | null>(null);
  let selectedRecordingAssets = $state<RecordingAssetPaths | null>(null);
  let activeRecordingSession = $state<RecordingSession | null>(null);
  let selectedTranscript = $state<TranscriptWithSegments | null>(null);
  let whisperModelStatus = $state<WhisperModelStatus | null>(null);
  let selectedSummary = $state<AiSummary | null>(null);
  let aiSettings = $state<AiSettings | null>(null);
  let selectedJobs = $state<ProcessingJob[]>([]);
  let isLoadingContext = $state(false);
  let runtimeError = $state("");
  let contextError = $state("");
  let draftTitle = $state("");
  let transcriptSearchQuery = $state("");
  let transcriptSearchResults = $state<TranscriptSearchResult[]>([]);
  let transcriptSearchError = $state("");
  let transcriptSearchTouched = $state(false);
  let transcriptSearchIndexSummary =
    $state<TranscriptSearchIndexSummary | null>(null);
  let captureStatus = $state<CaptureStatus | null>(null);
  let isCaptureLoading = $state(false);
  let isEncodingRetry = $state(false);
  let isTranscribing = $state(false);
  let isTranscriptSearching = $state(false);
  let isReindexingTranscriptSearch = $state(false);
  let isImportingWhisperModel = $state(false);
  let captureError = $state("");
  let whisperModelError = $state("");
  let transcriptionError = $state("");
  let aiSettingsError = $state("");
  let aiSummaryError = $state("");
  let selectedScreenSourceId = $state("");
  let selectedMicrophoneDeviceId = $state("");
  let selectedWhisperModel = $state("small.en");
  let aiEnabled = $state(false);
  let aiProvider = $state("openai_compatible");
  let aiModelName = $state("");
  let aiEndpointUrl = $state(defaultAiEndpointUrl);
  let aiApiKey = $state("");
  let aiUserNotes = $state("");
  let importWhisperModelPath = $state("");
  let isSavingAiSettings = $state(false);
  let isRunningAiSummary = $state(false);
  let retryingJobId = $state<string | null>(null);
  let pendingSeekMs = $state<number | null>(null);
  let playbackElement = $state<HTMLVideoElement | null>(null);

  const selectedRecording = $derived(
    recordings.find((recording) => recording.id === selectedRecordingId) ?? null,
  );

  const canStartCapture = $derived(
    captureStatus !== null &&
      activeRecordingSession === null &&
      captureStatus.screen.permissionState === "granted" &&
      captureStatus.displays.length > 0 &&
      captureStatus.microphones.length > 0,
  );

  const visibleJobs = $derived(
    selectedRecording ? selectedJobs : selectedJobs.slice(0, 3),
  );

  const selectedMediaUrl = $derived(
    selectedRecording?.status === "completed" && selectedRecordingAssets?.mediaPath
      ? convertFileSrc(selectedRecordingAssets.mediaPath)
      : "",
  );

  const selectedThumbnailUrl = $derived(
    selectedRecordingAssets?.thumbnailPath
      ? convertFileSrc(selectedRecordingAssets.thumbnailPath)
      : "",
  );

  const canRetrySelectedEncoding = $derived(
    selectedRecording?.status === "failed" &&
      selectedRecordingSession?.status === "stopped" &&
      !isEncodingRetry,
  );

  const canTranscribeSelectedRecording = $derived(
    selectedRecording?.status === "completed" &&
      whisperModelStatus?.exists === true &&
      whisperModelStatus?.binary.available === true &&
      !isTranscribing,
  );

  const aiSettingsReady = $derived(
    aiSettings?.enabled === true &&
      aiSettings?.hasApiKey === true &&
      (aiSettings?.modelName.trim().length ?? 0) > 0 &&
      (aiSettings?.endpointUrl.trim().length ?? 0) > 0,
  );

  const whisperModelOptions = $derived.by(() => {
    const names = new Set<string>([
      selectedWhisperModel,
      whisperModelStatus?.defaultModel ?? "small.en",
      ...commonWhisperModels,
      ...(whisperModelStatus?.availableModels.map((model) => model.name) ?? []),
    ]);

    return Array.from(names).filter(Boolean);
  });

  const whisperReadinessLabel = $derived.by(() => {
    if (!whisperModelStatus) return "Not checked";
    if (!whisperModelStatus.binary.available) return "Binary missing";
    if (!whisperModelStatus.exists) return "Model missing";

    return "Ready";
  });

  const transcriptionActionLabel = $derived.by(() => {
    if (isTranscribing) return "Transcribing";
    if (selectedTranscript?.transcript.status === "failed") return "Retry transcript";
    if (selectedTranscript?.transcript.status === "completed") return "Retranscribe";

    return "Transcribe";
  });

  const transcriptPreview = $derived.by(() => {
    if (selectedTranscript?.transcript.text) {
      return selectedTranscript.transcript.text;
    }

    if (selectedTranscript?.segments.length) {
      return selectedTranscript.segments.map((segment) => segment.text).join(" ");
    }

    return "";
  });

  const canRunAiSummary = $derived(
    selectedRecording?.status === "completed" &&
      selectedTranscript?.transcript.status === "completed" &&
      transcriptPreview.trim().length > 0 &&
      aiSettingsReady &&
      !isRunningAiSummary,
  );

  const aiSummaryActionLabel = $derived.by(() => {
    if (isRunningAiSummary) return "Generating";
    if (selectedSummary?.status === "failed") return "Retry AI";
    if (selectedSummary?.status === "completed") return "Regenerate AI";

    return "Generate AI";
  });

  const aiOutputSections = $derived.by(() => [
    {
      title: "Action items",
      items: parseAiOutputList(selectedSummary?.actionItemsJson ?? null),
    },
    {
      title: "Decisions",
      items: parseAiOutputList(selectedSummary?.decisionsJson ?? null),
    },
    {
      title: "Questions",
      items: parseAiOutputList(selectedSummary?.questionsJson ?? null),
    },
    {
      title: "Risks",
      items: parseAiOutputList(selectedSummary?.risksJson ?? null),
    },
    {
      title: "Chapters",
      items: parseAiOutputList(selectedSummary?.chaptersJson ?? null),
    },
  ]);

  const storagePaths = $derived(bootstrap?.storage.paths ?? null);

  onMount(() => {
    applyHashView();
    window.addEventListener("hashchange", applyHashView);
    const activeSessionTimer = window.setInterval(() => {
      if (captureUiState === "recording") {
        void refreshActiveSession();
      }
    }, 1000);
    const processingTimer = window.setInterval(() => {
      if (shouldPollProcessing()) {
        void refreshApp();
      }
    }, 2500);
    void refreshApp();

    return () => {
      window.removeEventListener("hashchange", applyHashView);
      window.clearInterval(activeSessionTimer);
      window.clearInterval(processingTimer);
    };
  });

  function applyHashView() {
    const hashView = window.location.hash.replace("#", "");
    if (isView(hashView)) {
      currentView = hashView;
    }
  }

  function isView(value: string): value is View {
    return navItems.some((item) => item.id === value);
  }

  function shouldPollProcessing() {
    if (nativeStatus === "loading") return false;

    return (
      recordings.some((recording) => recording.status === "processing") ||
      selectedJobs.some((job) => job.state === "queued" || job.state === "running")
    );
  }

  function setView(view: View) {
    currentView = view;
    window.history.replaceState(null, "", `#${view}`);
  }

  async function refreshApp() {
    nativeStatus = "loading";
    runtimeError = "";

    try {
      const [
        nextBootstrap,
        nextRecordings,
        allJobs,
        nextCaptureStatus,
        nextActiveSession,
        nextWhisperStatus,
        nextAiSettings,
      ] =
        await Promise.all([
          invoke<AppBootstrap>("app_bootstrap"),
          invoke<Recording[]>("list_recordings"),
          invoke<ProcessingJob[]>("list_processing_jobs", {
            state: null,
            recordingId: null,
          }),
          invoke<CaptureStatus>("capture_status"),
          invoke<RecordingSession | null>("active_recording_session"),
          invoke<WhisperModelStatus>("whisper_model_status", {
            modelName: selectedWhisperModel || null,
          }),
          invoke<AiSettings>("get_ai_settings"),
        ]);

      bootstrap = nextBootstrap;
      recordings = nextRecordings;
      activeRecordingSession = nextActiveSession;
      applyWhisperModelStatus(nextWhisperStatus);
      applyAiSettings(nextAiSettings);
      captureUiState = nextActiveSession ? "recording" : "idle";
      applyCaptureStatus(nextCaptureStatus);
      selectedJobs = selectedRecordingId
        ? await loadProcessingJobs(selectedRecordingId)
        : allJobs;

      if (!selectedRecordingId && nextRecordings.length > 0) {
        selectedRecordingId = nextRecordings[0].id;
        await loadRecordingContext(nextRecordings[0].id);
      } else if (selectedRecordingId) {
        await loadRecordingContext(selectedRecordingId);
      }

      nativeStatus = "ready";
    } catch (error) {
      nativeStatus = "error";
      runtimeError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to reach the native runtime.");
    }
  }

  async function loadProcessingJobs(recordingId: string) {
    return invoke<ProcessingJob[]>("list_processing_jobs", {
      state: null,
      recordingId,
    });
  }

  async function loadRecordingContext(recordingId: string) {
    isLoadingContext = true;
    contextError = "";

    try {
      const [transcript, summary, jobs, recordingSession, assetPaths] =
        await Promise.all([
          invoke<TranscriptWithSegments | null>("get_transcript_by_recording", {
            recordingId,
          }),
          invoke<AiSummary | null>("get_ai_summary_by_recording", {
            recordingId,
          }),
          loadProcessingJobs(recordingId),
          invoke<RecordingSession | null>("get_recording_session_by_recording", {
            recordingId,
          }),
          invoke<RecordingAssetPaths>("recording_asset_paths", {
            recordingId,
          }),
        ]);

      selectedTranscript = transcript;
      selectedSummary = summary;
      selectedJobs = jobs;
      selectedRecordingSession = recordingSession;
      selectedRecordingAssets = assetPaths;
    } catch (error) {
      selectedTranscript = null;
      selectedSummary = null;
      selectedJobs = [];
      selectedRecordingSession = null;
      selectedRecordingAssets = null;
      contextError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to load recording details.");
    } finally {
      isLoadingContext = false;
    }
  }

  async function selectRecording(recordingId: string) {
    selectedRecordingId = recordingId;
    setView("library");
    await loadRecordingContext(recordingId);
  }

  async function searchTranscripts() {
    const query = transcriptSearchQuery.trim();
    transcriptSearchTouched = true;
    transcriptSearchError = "";

    if (!query) {
      transcriptSearchResults = [];
      return;
    }

    isTranscriptSearching = true;

    try {
      transcriptSearchResults = await invoke<TranscriptSearchResult[]>(
        "search_transcripts",
        {
          input: {
            query,
            limit: 50,
          },
        },
      );
    } catch (error) {
      transcriptSearchResults = [];
      transcriptSearchError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to search local transcripts.");
    } finally {
      isTranscriptSearching = false;
    }
  }

  function handleTranscriptSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      void searchTranscripts();
    }
  }

  async function reindexTranscriptSearch() {
    isReindexingTranscriptSearch = true;
    transcriptSearchError = "";

    try {
      transcriptSearchIndexSummary =
        await invoke<TranscriptSearchIndexSummary>(
          "reindex_transcript_search",
        );

      if (transcriptSearchQuery.trim()) {
        await searchTranscripts();
      }
    } catch (error) {
      transcriptSearchError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to rebuild the transcript search index.");
    } finally {
      isReindexingTranscriptSearch = false;
    }
  }

  async function openSearchResult(result: TranscriptSearchResult) {
    pendingSeekMs = result.startMs;
    await selectRecording(result.recordingId);
    await tick();
    seekPlaybackToMs(result.startMs);
  }

  function applyCaptureStatus(status: CaptureStatus) {
    captureStatus = status;
    selectedScreenSourceId =
      status.selection.screenSourceId ??
      status.validatedConfig?.screenSource.id ??
      status.displays.find((display) => display.primary)?.id ??
      status.displays[0]?.id ??
      "";
    selectedMicrophoneDeviceId =
      status.selection.microphoneDeviceId ??
      status.validatedConfig?.microphone?.id ??
      status.microphones.find((microphone) => microphone.isDefault)?.id ??
      status.microphones[0]?.id ??
      "";
  }

  function applyWhisperModelStatus(status: WhisperModelStatus) {
    whisperModelStatus = status;
    selectedWhisperModel = status.selectedModel;
    whisperModelError = "";
  }

  function applyAiSettings(settings: AiSettings) {
    aiSettings = settings;
    aiEnabled = settings.enabled;
    aiProvider = settings.provider || "openai_compatible";
    aiModelName = settings.modelName;
    aiEndpointUrl = settings.endpointUrl || defaultAiEndpointUrl;
    aiApiKey = "";
    aiSettingsError = "";
  }

  async function refreshWhisperModelStatus() {
    whisperModelError = "";

    try {
      const status = await invoke<WhisperModelStatus>("whisper_model_status", {
        modelName: selectedWhisperModel || null,
      });
      applyWhisperModelStatus(status);
    } catch (error) {
      whisperModelError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to inspect Whisper model state.");
    }
  }

  async function importSelectedWhisperModel() {
    const sourcePath = importWhisperModelPath.trim();
    if (!sourcePath) return;

    isImportingWhisperModel = true;
    whisperModelError = "";

    try {
      const status = await invoke<WhisperModelStatus>("import_whisper_model", {
        input: {
          sourcePath,
          modelName: selectedWhisperModel || null,
        },
      });
      applyWhisperModelStatus(status);
      importWhisperModelPath = "";
    } catch (error) {
      whisperModelError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to import Whisper model.");
    } finally {
      isImportingWhisperModel = false;
    }
  }

  async function saveAiSettings() {
    isSavingAiSettings = true;
    aiSettingsError = "";

    try {
      const settings = await invoke<AiSettings>("save_ai_settings", {
        input: {
          enabled: aiEnabled,
          provider: aiProvider,
          modelName: aiModelName.trim() || null,
          endpointUrl: aiEndpointUrl.trim() || defaultAiEndpointUrl,
          apiKey: aiApiKey.trim() || null,
          clearApiKey: false,
        },
      });
      applyAiSettings(settings);
    } catch (error) {
      aiSettingsError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to save optional AI settings.");
    } finally {
      isSavingAiSettings = false;
    }
  }

  async function requestCapturePermissions() {
    isCaptureLoading = true;
    captureError = "";

    try {
      const status = await invoke<CaptureStatus>("request_capture_permissions");
      applyCaptureStatus(status);
    } catch (error) {
      captureError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to request capture permissions.");
    } finally {
      isCaptureLoading = false;
    }
  }

  async function saveCaptureSelection() {
    isCaptureLoading = true;
    captureError = "";

    try {
      const status = await invoke<CaptureStatus>("save_capture_selection", {
        input: {
          screenSourceId: selectedScreenSourceId || null,
          microphoneDeviceId: selectedMicrophoneDeviceId || null,
          includeMicrophone: true,
        },
      });
      applyCaptureStatus(status);
    } catch (error) {
      captureError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to save capture devices.");
    } finally {
      isCaptureLoading = false;
    }
  }

  async function refreshActiveSession() {
    try {
      const session = await invoke<RecordingSession | null>(
        "active_recording_session",
      );
      activeRecordingSession = session;

      if (session) {
        if (selectedRecordingId === session.recordingId) {
          selectedRecordingSession = session;
        }
      } else if (captureUiState === "recording") {
        captureUiState = "idle";
        await refreshApp();
      }
    } catch {
      // Avoid replacing the main runtime error while a recording is in progress.
    }
  }

  async function startDraftRecording() {
    nativeStatus = "loading";
    runtimeError = "";

    try {
      const { recording, session } = await invoke<RecordingSessionEnvelope>(
        "start_recording_session",
        {
          input: {
            title: draftTitle.trim() || null,
          },
        },
      );

      upsertRecording(recording);
      activeRecordingSession = session;
      selectedRecordingSession = session;
      selectedRecordingId = recording.id;
      captureUiState = "recording";
      currentView = "record";
      draftTitle = "";
      await loadRecordingContext(recording.id);
      nativeStatus = "ready";
    } catch (error) {
      await refreshApp();
      nativeStatus = "error";
      runtimeError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to start a local recording session.");
    }
  }

  async function stopDraftRecording() {
    if (!activeRecordingSession) return;

    nativeStatus = "loading";
    runtimeError = "";
    captureUiState = "processing";

    try {
      const { recording, session } = await invoke<RecordingSessionEnvelope>(
        "stop_recording_session",
        {
          input: {
            recordingId: activeRecordingSession.recordingId,
          },
        },
      );

      upsertRecording(recording);
      activeRecordingSession = null;
      selectedRecordingSession = session;
      selectedRecordingId = recording.id;
      captureUiState = "idle";
      await loadRecordingContext(recording.id);
      nativeStatus = "ready";
    } catch (error) {
      await refreshApp();
      nativeStatus = "error";
      runtimeError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to stop the local recording session.");
    }
  }

  async function retrySelectedEncoding() {
    if (!selectedRecording || !canRetrySelectedEncoding) return;

    const failedEncodingJob = selectedJobs.find(
      (job) =>
        job.kind === "encode_recording" &&
        (job.state === "failed" || job.interrupted),
    );

    if (failedEncodingJob) {
      await retryProcessingJob(failedEncodingJob);
      return;
    }

    nativeStatus = "loading";
    runtimeError = "";
    isEncodingRetry = true;

    try {
      const recording = await invoke<Recording>("encode_recording", {
        recordingId: selectedRecording.id,
      });

      upsertRecording(recording);
      selectedRecordingId = recording.id;
      await loadRecordingContext(recording.id);
      nativeStatus = "ready";
    } catch (error) {
      await refreshApp();
      nativeStatus = "error";
      runtimeError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to retry encoding.");
    } finally {
      isEncodingRetry = false;
    }
  }

  async function retryProcessingJob(job: ProcessingJob) {
    retryingJobId = job.id;
    runtimeError = "";
    contextError = "";

    try {
      await invoke<ProcessingJob>("retry_processing_job", {
        input: {
          jobId: job.id,
        },
      });

      if (job.recordingId) {
        selectedJobs = await loadProcessingJobs(job.recordingId);
        await loadRecordingContext(job.recordingId);
      } else {
        selectedJobs = await invoke<ProcessingJob[]>("list_processing_jobs", {
          state: null,
          recordingId: null,
        });
      }
    } catch (error) {
      contextError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to retry processing job.");
    } finally {
      retryingJobId = null;
    }
  }

  async function transcribeSelectedRecording() {
    if (!selectedRecording || !canTranscribeSelectedRecording) return;

    const recordingId = selectedRecording.id;
    nativeStatus = "loading";
    runtimeError = "";
    transcriptionError = "";
    isTranscribing = true;

    try {
      selectedTranscript = await invoke<TranscriptWithSegments>(
        "transcribe_recording",
        {
          input: {
            recordingId,
            modelName: selectedWhisperModel || null,
          },
        },
      );
      selectedJobs = await loadProcessingJobs(recordingId);
      nativeStatus = "ready";
    } catch (error) {
      await loadRecordingContext(recordingId);
      nativeStatus = "error";
      transcriptionError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to transcribe recording.");
    } finally {
      isTranscribing = false;
    }
  }

  async function summarizeSelectedRecording() {
    if (!selectedRecording || !canRunAiSummary) return;

    const recordingId = selectedRecording.id;
    aiSummaryError = "";
    isRunningAiSummary = true;

    try {
      selectedSummary = await invoke<AiSummary>("summarize_recording", {
        input: {
          recordingId,
          userNotes: aiUserNotes.trim() || null,
        },
      });
      selectedJobs = await loadProcessingJobs(recordingId);
    } catch (error) {
      await loadRecordingContext(recordingId);
      aiSummaryError =
        error instanceof Error
          ? error.message
          : String(error || "Unable to generate optional AI summary.");
    } finally {
      isRunningAiSummary = false;
    }
  }

  function upsertRecording(recording: Recording) {
    const rest = recordings.filter((item) => item.id !== recording.id);
    recordings = [recording, ...rest].sort((first, second) =>
      second.createdAt.localeCompare(first.createdAt),
    );
  }

  function formatDate(value: string | null) {
    if (!value) return "Not captured";

    const date = /^\d+$/.test(value)
      ? new Date(Number(value) * 1000)
      : new Date(value);
    if (Number.isNaN(date.getTime())) return value;

    return dateFormatter.format(date);
  }

  function formatDuration(durationMs: number | null) {
    if (!durationMs) return "Duration pending";

    const totalSeconds = Math.round(durationMs / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;

    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function formatBytes(byteCount: number | null) {
    if (!byteCount) return "No bytes written";

    const units = ["B", "KB", "MB", "GB"];
    let value = byteCount;
    let unitIndex = 0;

    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }

    return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
  }

  function formatResolution(session: RecordingSession | null) {
    if (!session?.width || !session.height) return "Pending";

    return `${session.width} x ${session.height}`;
  }

  function formatAudioSession(session: RecordingSession | null) {
    if (!session?.audioPath) return "Microphone disabled";

    const details = [];
    if (session.audioChannels) details.push(`${session.audioChannels} ch`);
    if (session.audioSampleRate) details.push(`${session.audioSampleRate} Hz`);
    if (session.audioSampleFormat) details.push(session.audioSampleFormat);

    return details.join(" / ") || "PCM pending";
  }

  function formatSegmentTime(ms: number) {
    const totalSeconds = Math.floor(ms / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;

    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function formatSearchRank(rank: number) {
    if (!Number.isFinite(rank)) return "Rank pending";

    const value =
      Math.abs(rank) < 0.001 ? rank.toExponential(2) : rank.toFixed(3);

    return `Rank ${value}`;
  }

  function jobKindLabel(kind: string) {
    return (
      jobKindLabels[kind] ??
      kind
        .split("_")
        .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
        .join(" ")
    );
  }

  function jobDisplayStatus(job: ProcessingJob) {
    if (job.id === retryingJobId) return "retrying";
    if (job.interrupted) return "interrupted";

    return job.state;
  }

  function jobStateLabel(job: ProcessingJob) {
    if (job.id === retryingJobId) return "Retrying";
    if (job.interrupted) return "Interrupted";
    if (job.state === "queued" && job.attempts > 0) return "Retry queued";
    if (job.state === "running" && job.attempts > 1) return "Retrying";

    return jobStateLabels[job.state];
  }

  function jobMeta(job: ProcessingJob) {
    const attemptCount =
      job.maxAttempts > 0
        ? `Attempt ${job.attempts}/${job.maxAttempts}`
        : `Attempt ${job.attempts}`;
    const lastError = job.lastErrorAt ? `Last error ${formatDate(job.lastErrorAt)}` : "";

    return [attemptCount, lastError || `Updated ${formatDate(job.updatedAt)}`]
      .filter(Boolean)
      .join(" / ");
  }

  function canRetryJob(job: ProcessingJob) {
    return (
      retryableJobKinds.has(job.kind) &&
      (job.state === "failed" || job.state === "cancelled" || job.interrupted) &&
      job.id !== retryingJobId
    );
  }

  function handleSelectedMediaLoaded() {
    if (pendingSeekMs !== null) {
      seekPlaybackToMs(pendingSeekMs);
    }
  }

  function seekPlaybackToMs(startMs: number) {
    if (!playbackElement) return;

    if (playbackElement.readyState < 1) {
      pendingSeekMs = startMs;
      return;
    }

    try {
      playbackElement.currentTime = Math.max(0, startMs / 1000);
      pendingSeekMs = null;
      void playbackElement.play().catch(() => {});
    } catch {
      pendingSeekMs = startMs;
    }
  }

  function formatConfidence(confidence: number | null) {
    if (confidence === null) return "";

    return `${Math.round(confidence * 100)}%`;
  }

  function permissionLabel(state: CapturePermissionState) {
    switch (state) {
      case "granted":
        return "Granted";
      case "prompt_required":
        return "Needs approval";
      case "denied":
        return "Denied";
      case "unavailable":
        return "Unavailable";
      case "unknown":
        return "Not verified";
    }
  }

  function selectedDisplayLabel() {
    const display = captureStatus?.displays.find(
      (source) => source.id === selectedScreenSourceId,
    );

    return display?.title ?? "No display selected";
  }

  function selectedMicrophoneLabel() {
    const microphone = captureStatus?.microphones.find(
      (device) => device.id === selectedMicrophoneDeviceId,
    );

    return microphone?.name ?? "No microphone selected";
  }

  function microphoneDetail(microphone: MicrophoneDevice) {
    const details = [];

    if (microphone.isDefault) details.push("Default");
    if (microphone.channels) details.push(`${microphone.channels} ch`);
    if (microphone.sampleRate) details.push(`${microphone.sampleRate} Hz`);

    return details.join(" / ") || "Input device";
  }

  function pathTail(path: string | null) {
    if (!path) return "Pending";

    const parts = path.split(/[\\/]/).filter(Boolean);
    return parts.slice(-2).join("/");
  }

  function parseAiOutputList(jsonText: string | null): AiOutputItem[] {
    if (!jsonText) return [];

    try {
      const parsed: unknown = JSON.parse(jsonText);
      const items = Array.isArray(parsed) ? parsed : [parsed];

      return items
        .map(normalizeAiOutputItem)
        .filter((item) => item.primary.length > 0);
    } catch {
      return [{ primary: jsonText, detail: "" }];
    }
  }

  function normalizeAiOutputItem(item: unknown): AiOutputItem {
    if (typeof item === "string") {
      return { primary: item, detail: "" };
    }

    if (typeof item === "number" || typeof item === "boolean") {
      return { primary: String(item), detail: "" };
    }

    if (!item || typeof item !== "object") {
      return { primary: "", detail: "" };
    }

    const record = item as Record<string, unknown>;
    const primary =
      firstString(record, [
        "title",
        "text",
        "summary",
        "question",
        "risk",
        "decision",
        "action",
        "description",
      ]) ?? JSON.stringify(item);
    const details = [
      formatAiTimeRange(record),
      firstString(record, ["owner", "due", "status", "priority"]),
      firstString(record, ["details", "rationale", "reason"]),
    ].filter(Boolean);

    return {
      primary,
      detail: details.join(" / "),
    };
  }

  function firstString(record: Record<string, unknown>, keys: string[]) {
    for (const key of keys) {
      const value = record[key];
      if (typeof value === "string" && value.trim()) {
        return value.trim();
      }
    }

    return null;
  }

  function formatAiTimeRange(record: Record<string, unknown>) {
    const start = numericValue(record.start_ms ?? record.startMs);
    const end = numericValue(record.end_ms ?? record.endMs);

    if (start === null && end === null) return "";
    if (start !== null && end !== null) {
      return `${formatSegmentTime(start)} - ${formatSegmentTime(end)}`;
    }

    return formatSegmentTime(start ?? end ?? 0);
  }

  function numericValue(value: unknown) {
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  }
</script>

<svelte:head>
  <title>Metafy Desktop Library</title>
</svelte:head>

<section class="desktop-workspace" aria-label="Metafy Desktop workspace">
  <header class="workspace-header">
    <div>
      <p class="eyebrow">Local recorder</p>
      <h1>Recording library</h1>
    </div>

    <div class="runtime-cluster" aria-live="polite">
      <span class="runtime-pill" data-state={nativeStatus}>
        {nativeStatus === "ready"
          ? "Native ready"
          : nativeStatus === "loading"
            ? "Syncing"
            : nativeStatus === "error"
              ? "Runtime offline"
              : "Waiting"}
      </span>
      <button class="ghost-button" type="button" onclick={refreshApp}>
        Refresh
      </button>
    </div>
  </header>

  {#if runtimeError}
    <section class="notice error" aria-label="Native runtime error">
      <strong>Native command unavailable</strong>
      <span>{runtimeError}</span>
    </section>
  {/if}

  <div class="app-grid">
    <aside class="section-nav" aria-label="Workspace navigation">
      {#each navItems as item (item.id)}
        <button
          class="section-link"
          class:active={currentView === item.id}
          type="button"
          onclick={() => setView(item.id)}
          aria-current={currentView === item.id ? "page" : undefined}
        >
          <span>{item.label}</span>
          <small>{item.detail}</small>
        </button>
      {/each}

      <div class="local-summary">
        <span class="status-dot" aria-hidden="true"></span>
        <div>
          <strong>{bootstrap?.storage.recordingCount ?? recordings.length}</strong>
          <span>local recordings</span>
        </div>
      </div>
    </aside>

    <main class="primary-surface">
      {#if currentView === "record"}
        <section class="recording-panel" aria-labelledby="recording-controls">
          <div class="section-heading">
            <div>
              <p class="section-label">Session</p>
              <h2 id="recording-controls">Recording controls</h2>
            </div>
            <span class="status-chip" data-status={captureUiState}>
              {captureUiState === "recording"
                ? "Recording"
                : captureUiState === "processing"
                  ? "Processing"
                  : "Idle"}
            </span>
          </div>

          <div class="recording-console" data-state={captureUiState}>
            <div class="pulse-ring" aria-hidden="true">
              <span></span>
            </div>

            <div class="recording-copy">
              <strong>
                {captureUiState === "recording"
                  ? "Writing temporary media"
                  : captureUiState === "processing"
                    ? "Session queued for processing"
                    : "Ready for a local session"}
              </strong>
              <span>
                {captureUiState === "recording"
                  ? "Screen frames and microphone PCM are being written under app temp."
                  : captureUiState === "processing"
                    ? "Encoding and transcription states will attach here."
                    : "SQLite session state is persisted before capture starts."}
              </span>
            </div>

            <div class="control-stack">
              {#if captureUiState === "idle"}
                <label>
                  <span>Title</span>
                  <input
                    bind:value={draftTitle}
                    type="text"
                    placeholder="Untitled recording"
                    autocomplete="off"
                  />
                </label>
                <button
                  class="primary-action"
                  type="button"
                  onclick={startDraftRecording}
                  disabled={
                    nativeStatus === "loading" ||
                    isCaptureLoading ||
                    !canStartCapture
                  }
                >
                  Start session
                </button>
              {:else if captureUiState === "recording"}
                <button
                  class="primary-action danger"
                  type="button"
                  onclick={stopDraftRecording}
                  disabled={nativeStatus === "loading"}
                >
                  Stop session
                </button>
              {/if}
            </div>
          </div>

          {#if activeRecordingSession}
            <dl class="session-metrics" aria-label="Active recording session metrics">
              <div>
                <dt>Duration</dt>
                <dd>{formatDuration(activeRecordingSession.durationMs)}</dd>
              </div>
              <div>
                <dt>Frames</dt>
                <dd>{activeRecordingSession.frameCount}</dd>
              </div>
              <div>
                <dt>Resolution</dt>
                <dd>{formatResolution(activeRecordingSession)}</dd>
              </div>
              <div>
                <dt>Audio</dt>
                <dd>{formatBytes(activeRecordingSession.audioByteCount)}</dd>
              </div>
            </dl>
          {/if}

          <div class="capture-config" aria-label="Capture device configuration">
            <div class="capture-status-grid">
              <section aria-labelledby="screen-permission">
                <div class="panel-heading-inline">
                  <h3 id="screen-permission">Screen</h3>
                  <span
                    class="status-chip"
                    data-status={captureStatus?.screen.permissionState ?? "unknown"}
                  >
                    {permissionLabel(
                      captureStatus?.screen.permissionState ?? "unknown",
                    )}
                  </span>
                </div>
                <p>{selectedDisplayLabel()}</p>
                {#if captureStatus?.screen.error}
                  <small>{captureStatus.screen.error}</small>
                {/if}
              </section>

              <section aria-labelledby="microphone-permission">
                <div class="panel-heading-inline">
                  <h3 id="microphone-permission">Microphone</h3>
                  <span
                    class="status-chip"
                    data-status={captureStatus?.microphone.permissionState ?? "unknown"}
                  >
                    {permissionLabel(
                      captureStatus?.microphone.permissionState ?? "unknown",
                    )}
                  </span>
                </div>
                <p>{selectedMicrophoneLabel()}</p>
                {#if captureStatus?.microphone.error}
                  <small>{captureStatus.microphone.error}</small>
                {/if}
              </section>
            </div>

            <div class="capture-controls">
              <label>
                <span>Display</span>
                <select
                  bind:value={selectedScreenSourceId}
                  onchange={saveCaptureSelection}
                  disabled={
                    isCaptureLoading ||
                    activeRecordingSession !== null ||
                    captureStatus?.displays.length === 0
                  }
                >
                  <option value="">No display available</option>
                  {#each captureStatus?.displays ?? [] as display (display.id)}
                    <option value={display.id}>
                      {display.title}{display.primary ? " (Primary)" : ""}
                    </option>
                  {/each}
                </select>
              </label>

              <label>
                <span>Microphone</span>
                <select
                  bind:value={selectedMicrophoneDeviceId}
                  onchange={saveCaptureSelection}
                  disabled={
                    isCaptureLoading ||
                    activeRecordingSession !== null ||
                    captureStatus?.microphones.length === 0
                  }
                >
                  <option value="">No microphone available</option>
                  {#each captureStatus?.microphones ?? [] as microphone (microphone.id)}
                    <option value={microphone.id}>
                      {microphone.name} - {microphoneDetail(microphone)}
                    </option>
                  {/each}
                </select>
              </label>

              <button
                class="ghost-button"
                type="button"
                onclick={requestCapturePermissions}
                disabled={
                  isCaptureLoading ||
                  nativeStatus === "loading" ||
                  activeRecordingSession !== null
                }
              >
                {isCaptureLoading ? "Checking" : "Request access"}
              </button>
            </div>

            {#if captureError}
              <p class="failure-message">{captureError}</p>
            {:else if captureStatus?.validationErrors.length}
              <ul class="capture-errors" aria-label="Capture validation errors">
                {#each captureStatus.validationErrors as validationError}
                  <li>{validationError}</li>
                {/each}
              </ul>
            {/if}
          </div>
        </section>

        <section class="queue-panel" aria-labelledby="state-coverage">
          <div class="section-heading compact">
            <div>
              <p class="section-label">States</p>
              <h2 id="state-coverage">Recording lifecycle</h2>
            </div>
          </div>

          <div class="state-grid">
            {#each lifecycleStates as state (state.status)}
              <div class="state-cell">
                <span class="status-chip" data-status={state.status}>
                  {statusLabels[state.status]}
                </span>
                <p>{state.description}</p>
              </div>
            {/each}
          </div>
        </section>
      {:else if currentView === "library"}
        <section class="library-panel" aria-labelledby="library-heading">
          <div class="section-heading">
            <div>
              <p class="section-label">SQLite</p>
              <h2 id="library-heading">Local recordings</h2>
            </div>
            <span>{recordings.length} total</span>
          </div>

          <div class="library-layout">
            <div class="recording-list" aria-label="Persisted recordings">
              {#if nativeStatus === "loading" && recordings.length === 0}
                <p class="empty-state">Loading local recordings...</p>
              {:else if recordings.length === 0}
                <p class="empty-state">No local recordings yet.</p>
              {:else}
                {#each recordings as recording (recording.id)}
                  <button
                    class="recording-row"
                    class:selected={selectedRecordingId === recording.id}
                    type="button"
                    onclick={() => selectRecording(recording.id)}
                  >
                    <span class="row-title">{recording.title}</span>
                    <span class="row-meta">
                      {formatDate(recording.capturedAt ?? recording.createdAt)}
                    </span>
                    <span class="status-chip" data-status={recording.status}>
                      {statusLabels[recording.status]}
                    </span>
                  </button>
                {/each}
              {/if}
            </div>

            <section class="detail-panel" aria-label="Recording detail">
              {#if selectedRecording}
                <div class="detail-header">
                  <div>
                    <p class="section-label">Selected recording</p>
                    <h3>{selectedRecording.title}</h3>
                  </div>
                  <div class="detail-actions">
                    {#if selectedRecording.status === "completed"}
                      <button
                        class="ghost-button"
                        type="button"
                        onclick={transcribeSelectedRecording}
                        disabled={
                          !canTranscribeSelectedRecording ||
                          nativeStatus === "loading"
                        }
                      >
                        {transcriptionActionLabel}
                      </button>
                    {/if}
                    {#if selectedRecording.status === "failed" && selectedRecordingSession?.status === "stopped"}
                      <button
                        class="ghost-button"
                        type="button"
                        onclick={retrySelectedEncoding}
                        disabled={!canRetrySelectedEncoding || nativeStatus === "loading"}
                      >
                        {isEncodingRetry ? "Retrying" : "Retry encode"}
                      </button>
                    {/if}
                    <span class="status-chip" data-status={selectedRecording.status}>
                      {statusLabels[selectedRecording.status]}
                    </span>
                  </div>
                </div>

                {#if selectedMediaUrl}
                  <div class="player-surface">
                    <!-- svelte-ignore a11y_media_has_caption -->
                    <video
                      bind:this={playbackElement}
                      controls
                      onloadedmetadata={handleSelectedMediaLoaded}
                      preload="metadata"
                      poster={selectedThumbnailUrl}
                      src={selectedMediaUrl}
                    ></video>
                    <div class="media-path">
                      <strong>{pathTail(selectedRecording.mediaPath)}</strong>
                      <span>{formatResolution(selectedRecordingSession)}</span>
                    </div>
                  </div>
                {:else}
                  <div class="player-placeholder">
                    <div>
                      <strong>
                        {selectedRecording.status === "processing"
                          ? "Encoding MP4"
                          : selectedRecordingSession
                            ? "Temporary media"
                            : "MP4 playback"}
                      </strong>
                      <span>{pathTail(selectedRecording.mediaPath)}</span>
                    </div>
                    <p>
                      {selectedRecording.status === "failed" &&
                      selectedRecordingSession?.status === "stopped"
                        ? "Encoding failed, but the temporary capture files are preserved for retry."
                        : selectedRecording.status === "processing"
                          ? "FFmpeg is preparing the local MP4 and thumbnail."
                          : selectedRecordingSession
                            ? "Raw screen frames and microphone PCM are ready for the encoding step."
                            : "Playback appears after encoding writes a media path."}
                    </p>
                  </div>
                {/if}

                <dl class="metadata-grid">
                  <div>
                    <dt>Captured</dt>
                    <dd>{formatDate(selectedRecording.capturedAt)}</dd>
                  </div>
                  <div>
                    <dt>Duration</dt>
                    <dd>{formatDuration(selectedRecording.durationMs)}</dd>
                  </div>
                  <div>
                    <dt>Directory</dt>
                    <dd>{pathTail(selectedRecording.recordingDirectory)}</dd>
                  </div>
                  <div>
                    <dt>Updated</dt>
                    <dd>{formatDate(selectedRecording.updatedAt)}</dd>
                  </div>
                  <div>
                    <dt>Media</dt>
                    <dd>{pathTail(selectedRecording.mediaPath)}</dd>
                  </div>
                  <div>
                    <dt>Thumbnail</dt>
                    <dd>{pathTail(selectedRecording.thumbnailPath)}</dd>
                  </div>
                </dl>

                {#if selectedRecordingSession}
                  <dl class="metadata-grid session-detail-grid">
                    <div>
                      <dt>Session</dt>
                      <dd>
                        {sessionStatusLabels[selectedRecordingSession.status]}
                      </dd>
                    </div>
                    <div>
                      <dt>Resolution</dt>
                      <dd>{formatResolution(selectedRecordingSession)}</dd>
                    </div>
                    <div>
                      <dt>Frame rate</dt>
                      <dd>{selectedRecordingSession.frameRate} fps</dd>
                    </div>
                    <div>
                      <dt>Frames</dt>
                      <dd>{selectedRecordingSession.frameCount}</dd>
                    </div>
                    <div>
                      <dt>Video temp</dt>
                      <dd>{pathTail(selectedRecordingSession.videoPath)}</dd>
                    </div>
                    <div>
                      <dt>Audio temp</dt>
                      <dd>{pathTail(selectedRecordingSession.audioPath)}</dd>
                    </div>
                    <div>
                      <dt>Audio format</dt>
                      <dd>{formatAudioSession(selectedRecordingSession)}</dd>
                    </div>
                    <div>
                      <dt>Metadata</dt>
                      <dd>{pathTail(selectedRecordingSession.metadataPath)}</dd>
                    </div>
                  </dl>
                {/if}

                {#if selectedRecording.failureMessage}
                  <p class="failure-message">{selectedRecording.failureMessage}</p>
                {/if}

                <div class="split-panels">
                  <section aria-labelledby="transcript-panel">
                    <div class="panel-heading-inline">
                      <h4 id="transcript-panel">Transcript</h4>
                      {#if selectedTranscript}
                        <span
                          class="status-chip"
                          data-status={selectedTranscript.transcript.status}
                        >
                          {transcriptStatusLabels[selectedTranscript.transcript.status]}
                        </span>
                      {/if}
                    </div>

                    {#if isLoadingContext}
                      <p class="empty-state">Loading transcript...</p>
                    {:else if transcriptionError}
                      <p class="failure-message">{transcriptionError}</p>
                    {:else if selectedTranscript?.transcript.status === "failed"}
                      <p class="failure-message">
                        {selectedTranscript.transcript.failureMessage ??
                          "Transcription failed."}
                      </p>
                    {:else if transcriptPreview}
                      <dl class="transcript-meta">
                        <div>
                          <dt>Model</dt>
                          <dd>
                            {selectedTranscript?.transcript.modelName ??
                              selectedWhisperModel}
                          </dd>
                        </div>
                        <div>
                          <dt>Raw JSON</dt>
                          <dd>
                            {pathTail(
                              selectedTranscript?.transcript.rawJsonPath ?? null,
                            )}
                          </dd>
                        </div>
                      </dl>
                      <p class="transcript-copy">{transcriptPreview}</p>
                      {#if selectedTranscript?.segments.length}
                        <ul class="segment-list">
                          {#each selectedTranscript.segments as segment (segment.id)}
                            <li>
                              <span class="segment-time">
                                {formatSegmentTime(segment.startMs)}
                              </span>
                              <p>
                                {segment.text}
                                {#if segment.confidence !== null}
                                  <small>{formatConfidence(segment.confidence)}</small>
                                {/if}
                              </p>
                            </li>
                          {/each}
                        </ul>
                      {/if}
                    {:else if selectedTranscript?.transcript.status === "processing"}
                      <p class="empty-state">Whisper transcription is running.</p>
                    {:else}
                      <p class="empty-state">
                        {whisperModelStatus?.exists === false
                          ? `Add ${whisperModelStatus.expectedFileName} before transcription.`
                          : whisperModelStatus?.binary.available === false
                            ? `Install whisper.cpp or set ${whisperModelStatus.binary.envVar}.`
                            : "Transcript segments will appear after Whisper finishes."}
                      </p>
                    {/if}
                  </section>

                  <section aria-labelledby="job-panel">
                    <div class="panel-heading-inline">
                      <h4 id="job-panel">Jobs</h4>
                      <span>{selectedJobs.length}</span>
                    </div>

                    {#if contextError}
                      <p class="failure-message">{contextError}</p>
                    {:else if visibleJobs.length === 0}
                      <p class="empty-state">No processing jobs for this recording.</p>
                    {:else}
                      <ul class="job-list">
                        {#each visibleJobs as job (job.id)}
                          <li>
                            <div class="job-main">
                              <span>{jobKindLabel(job.kind)}</span>
                              <small>{jobMeta(job)}</small>
                              {#if job.errorMessage}
                                <small class="job-error">{job.errorMessage}</small>
                              {/if}
                            </div>
                            <div class="job-actions">
                              <span
                                class="status-chip"
                                data-status={jobDisplayStatus(job)}
                              >
                                {jobStateLabel(job)}
                              </span>
                              {#if canRetryJob(job)}
                                <button
                                  class="ghost-button compact"
                                  type="button"
                                  onclick={() => retryProcessingJob(job)}
                                  disabled={retryingJobId !== null}
                                >
                                  Retry
                                </button>
                              {/if}
                            </div>
                          </li>
                        {/each}
                      </ul>
                    {/if}
                  </section>
                </div>

                <section class="ai-panel" aria-labelledby="ai-panel">
                  <div class="panel-heading-inline">
                    <h4 id="ai-panel">Optional AI</h4>
                    <div class="detail-actions">
                      {#if selectedSummary}
                        <span class="status-chip" data-status={selectedSummary.status}>
                          {aiStatusLabels[selectedSummary.status]}
                        </span>
                      {/if}
                      <button
                        class="ghost-button"
                        type="button"
                        onclick={summarizeSelectedRecording}
                        disabled={!canRunAiSummary}
                      >
                        {aiSummaryActionLabel}
                      </button>
                    </div>
                  </div>

                  {#if aiSummaryError}
                    <p class="failure-message">{aiSummaryError}</p>
                  {:else if selectedSummary?.status === "failed"}
                    <p class="failure-message">
                      {selectedSummary.failureMessage ?? "Optional AI failed."}
                    </p>
                  {/if}

                  {#if !aiSettings?.enabled}
                    <p class="empty-state">
                      Optional AI is off. Enable and configure it in Settings to
                      send transcript-only payloads to your provider.
                    </p>
                  {:else if !aiSettingsReady}
                    <p class="empty-state">
                      Save a provider endpoint, model, and API key before running
                      optional AI.
                    </p>
                  {:else if selectedTranscript?.transcript.status !== "completed"}
                    <p class="empty-state">
                      Complete a local Whisper transcript before running optional
                      AI.
                    </p>
                  {:else}
                    <label class="ai-notes">
                      <span>Notes for this run</span>
                      <textarea
                        bind:value={aiUserNotes}
                        rows="3"
                        placeholder="Optional context to include with the transcript"
                      ></textarea>
                    </label>
                  {/if}

                  {#if selectedSummary?.summaryText}
                    <p class="transcript-copy">{selectedSummary.summaryText}</p>
                    {#if aiOutputSections.some((section) => section.items.length > 0)}
                      <div class="ai-output-grid">
                        {#each aiOutputSections as section (section.title)}
                          {#if section.items.length > 0}
                            <section aria-label={section.title}>
                              <h5>{section.title}</h5>
                              <ul class="ai-output-list">
                                {#each section.items as item, index (index)}
                                  <li>
                                    <strong>{item.primary}</strong>
                                    {#if item.detail}
                                      <small>{item.detail}</small>
                                    {/if}
                                  </li>
                                {/each}
                              </ul>
                            </section>
                          {/if}
                        {/each}
                      </div>
                    {/if}
                  {:else if selectedSummary?.status === "processing" || isRunningAiSummary}
                    <p class="empty-state">AI summary generation is running.</p>
                  {:else if aiSettings?.enabled && aiSettingsReady}
                    <p class="empty-state">
                      Transcript-only AI output will appear here after generation.
                    </p>
                  {/if}
                </section>
              {:else}
                <p class="empty-state">Select a recording to view local metadata.</p>
              {/if}
            </section>
          </div>
        </section>
      {:else if currentView === "search"}
        <section class="search-panel" aria-labelledby="search-heading">
          <div class="section-heading">
            <div>
              <p class="section-label">Local index</p>
              <h2 id="search-heading">Transcript search</h2>
            </div>
            <span>
              {transcriptSearchIndexSummary
                ? `${transcriptSearchIndexSummary.indexedSegmentCount} indexed`
                : `${transcriptSearchResults.length} results`}
            </span>
          </div>

          <div class="search-controls">
            <label>
              <span>Search transcript segments</span>
              <input
                bind:value={transcriptSearchQuery}
                type="search"
                placeholder="Keyword or phrase"
                onkeydown={handleTranscriptSearchKeydown}
              />
            </label>

            <button
              class="primary-action"
              type="button"
              onclick={searchTranscripts}
              disabled={isTranscriptSearching || !transcriptSearchQuery.trim()}
            >
              {isTranscriptSearching ? "Searching" : "Search"}
            </button>

            <button
              class="ghost-button"
              type="button"
              onclick={reindexTranscriptSearch}
              disabled={isReindexingTranscriptSearch}
            >
              {isReindexingTranscriptSearch ? "Reindexing" : "Reindex"}
            </button>
          </div>

          <div class="search-results" aria-label="Transcript search results">
            {#if transcriptSearchError}
              <p class="failure-message">{transcriptSearchError}</p>
            {:else if isTranscriptSearching}
              <p class="empty-state">Searching local transcript segments...</p>
            {:else if !transcriptSearchTouched}
              <p class="empty-state">
                Search completed transcripts by keyword. Results stay local and
                open at the matching timestamp.
              </p>
            {:else if transcriptSearchResults.length === 0}
              <p class="empty-state">No transcript segments match this search.</p>
            {:else}
              {#each transcriptSearchResults as result (result.segmentId)}
                <button
                  class="search-row transcript-search-row"
                  type="button"
                  onclick={() => openSearchResult(result)}
                >
                  <span>
                    <strong>{result.recordingTitle}</strong>
                    <small>
                      {formatSegmentTime(result.startMs)} - {formatDate(
                        result.capturedAt ?? result.createdAt,
                      )} - {formatSearchRank(result.rank)}
                    </small>
                    <p>{result.snippet || result.text}</p>
                  </span>
                  <span class="status-chip" data-status="completed">
                    {formatSegmentTime(result.startMs)}
                  </span>
                </button>
              {/each}
            {/if}
          </div>
        </section>
      {:else}
        <section class="settings-panel" aria-labelledby="settings-heading">
          <div class="section-heading">
            <div>
              <p class="section-label">Defaults</p>
              <h2 id="settings-heading">Local settings</h2>
            </div>
            <span>Network disabled by default</span>
          </div>

          <div class="settings-grid">
            <section aria-labelledby="storage-location">
              <h3 id="storage-location">Storage location</h3>
              {#if storagePaths}
                <dl class="path-list">
                  <div>
                    <dt>Root</dt>
                    <dd>{storagePaths.root}</dd>
                  </div>
                  <div>
                    <dt>Database</dt>
                    <dd>{storagePaths.databaseFile}</dd>
                  </div>
                  <div>
                    <dt>Recordings</dt>
                    <dd>{storagePaths.recordingsDirectory}</dd>
                  </div>
                  <div>
                    <dt>Whisper models</dt>
                    <dd>{storagePaths.whisperModelsDirectory}</dd>
                  </div>
                </dl>
              {:else}
                <p class="empty-state">Storage paths load from the Tauri runtime.</p>
              {/if}
            </section>

            <section aria-labelledby="processing-settings">
              <h3 id="processing-settings">Processing</h3>
              <div class="settings-list">
                <label class="setting-row">
                  <span>
                    <strong>Whisper model</strong>
                    <small>{whisperReadinessLabel}</small>
                  </span>
                  <select
                    bind:value={selectedWhisperModel}
                    onchange={refreshWhisperModelStatus}
                    disabled={isTranscribing || isImportingWhisperModel}
                  >
                    {#each whisperModelOptions as modelName (modelName)}
                      <option value={modelName}>{modelName}</option>
                    {/each}
                  </select>
                </label>

                <dl class="model-status-grid" aria-label="Whisper model status">
                  <div>
                    <dt>Model file</dt>
                    <dd>
                      <span
                        class="status-chip"
                        data-status={whisperModelStatus?.exists ? "completed" : "failed"}
                      >
                        {whisperModelStatus?.exists ? "Found" : "Missing"}
                      </span>
                    </dd>
                    <small>
                      {whisperModelStatus?.expectedFileName ?? "ggml-small.en.bin"}
                    </small>
                  </div>

                  <div>
                    <dt>whisper.cpp</dt>
                    <dd>
                      <span
                        class="status-chip"
                        data-status={
                          whisperModelStatus?.binary.available
                            ? "completed"
                            : "failed"
                        }
                      >
                        {whisperModelStatus?.binary.available ? "Found" : "Missing"}
                      </span>
                    </dd>
                    <small>
                      {whisperModelStatus?.binary.path ??
                        whisperModelStatus?.binary.envVar ??
                        "METAFY_WHISPER_CPP_PATH"}
                    </small>
                  </div>
                </dl>

                <div class="import-row">
                  <label>
                    <span>Import model file</span>
                    <input
                      bind:value={importWhisperModelPath}
                      type="text"
                      placeholder={whisperModelStatus?.modelPath ??
                        "Path to ggml-small.en.bin"}
                      autocomplete="off"
                    />
                  </label>
                  <button
                    class="ghost-button"
                    type="button"
                    onclick={importSelectedWhisperModel}
                    disabled={
                      isImportingWhisperModel || !importWhisperModelPath.trim()
                    }
                  >
                    {isImportingWhisperModel ? "Importing" : "Import"}
                  </button>
                </div>

                {#if whisperModelError}
                  <p class="failure-message">{whisperModelError}</p>
                {:else}
                  <p class="empty-state">
                    Models directory: {whisperModelStatus?.modelsDirectory ??
                      storagePaths?.whisperModelsDirectory ??
                      "Pending"}
                  </p>
                {/if}

                <div class="ai-settings-block">
                  <label class="setting-row">
                    <span>
                      <strong>Optional AI</strong>
                      <small>
                        {aiSettings?.enabled
                          ? "Transcript-only network calls enabled"
                          : "Disabled by default"}
                      </small>
                    </span>
                    <input bind:checked={aiEnabled} type="checkbox" />
                  </label>

                  <div class="ai-settings-grid">
                    <label>
                      <span>Provider</span>
                      <select bind:value={aiProvider}>
                        <option value="openai_compatible">OpenAI compatible</option>
                      </select>
                    </label>

                    <label>
                      <span>Model</span>
                      <input
                        bind:value={aiModelName}
                        type="text"
                        placeholder="Provider model name"
                        autocomplete="off"
                      />
                    </label>
                  </div>

                  <label>
                    <span>Chat completions endpoint</span>
                    <input
                      bind:value={aiEndpointUrl}
                      type="url"
                      placeholder={defaultAiEndpointUrl}
                      autocomplete="off"
                    />
                  </label>

                  <div class="import-row">
                    <label>
                      <span>API key</span>
                      <input
                        bind:value={aiApiKey}
                        type="password"
                        placeholder={aiSettings?.hasApiKey
                          ? "Saved locally"
                          : "Required before enable"}
                        autocomplete="off"
                      />
                    </label>
                    <button
                      class="ghost-button"
                      type="button"
                      onclick={saveAiSettings}
                      disabled={isSavingAiSettings}
                    >
                      {isSavingAiSettings ? "Saving" : "Save AI"}
                    </button>
                  </div>

                  {#if aiSettingsError}
                    <p class="failure-message">{aiSettingsError}</p>
                  {:else}
                    <p class="empty-state">
                      Payloads include transcript text, recording metadata, and
                      optional notes only. Raw media and file paths are rejected
                      before provider calls.
                    </p>
                  {/if}
                </div>

                <label class="setting-row">
                  <span>
                    <strong>Disable core network</strong>
                    <small>Disabled for recording, playback, search</small>
                  </span>
                  <input type="checkbox" disabled checked />
                </label>
              </div>
            </section>
          </div>
        </section>
      {/if}
    </main>
  </div>
</section>

<style>
  .desktop-workspace {
    display: grid;
    gap: 24px;
  }

  .workspace-header,
  .section-heading,
  .detail-header,
  .panel-heading-inline,
  .runtime-cluster {
    display: flex;
    align-items: center;
  }

  .workspace-header,
  .section-heading,
  .detail-header {
    justify-content: space-between;
    gap: 18px;
  }

  .workspace-header h1,
  .section-heading h2,
  .detail-header h3,
  .settings-grid h3,
  .panel-heading-inline h3,
  .panel-heading-inline h4 {
    margin: 0;
    color: #171914;
    letter-spacing: 0;
  }

  .workspace-header h1 {
    font-size: clamp(2rem, 3vw, 3.35rem);
    line-height: 0.95;
  }

  .section-heading h2 {
    font-size: 1.18rem;
    line-height: 1.2;
  }

  .detail-header h3 {
    max-width: 32rem;
    overflow-wrap: anywhere;
    font-size: 1.35rem;
    line-height: 1.2;
  }

  .settings-grid h3,
  .panel-heading-inline h3,
  .panel-heading-inline h4 {
    font-size: 0.95rem;
    line-height: 1.2;
  }

  .eyebrow,
  .section-label {
    margin: 0 0 6px;
    color: #596052;
    font-size: 0.72rem;
    font-weight: 750;
    letter-spacing: 0;
    text-transform: uppercase;
  }

  .runtime-cluster {
    gap: 10px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .runtime-pill,
  .status-chip {
    display: inline-flex;
    min-height: 28px;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    padding: 5px 10px;
    font-size: 0.76rem;
    font-weight: 740;
    white-space: nowrap;
  }

  .runtime-pill {
    background: #e7ece2;
    color: #304535;
  }

  .runtime-pill[data-state="loading"] {
    background: #ece7d9;
    color: #604a18;
  }

  .runtime-pill[data-state="error"] {
    background: #f5dddd;
    color: #8d2727;
  }

  .ghost-button,
  .primary-action,
  .section-link,
  .recording-row,
  .search-row {
    border: 0;
    letter-spacing: 0;
    cursor: pointer;
  }

  .ghost-button {
    min-height: 36px;
    border: 1px solid #cfd7cb;
    border-radius: 8px;
    background: #fbfcf8;
    color: #252820;
    padding: 0 13px;
    font-weight: 680;
  }

  .ghost-button.compact {
    min-height: 30px;
    padding: 0 10px;
    font-size: 0.76rem;
  }

  .app-grid {
    display: grid;
    grid-template-columns: minmax(176px, 0.18fr) minmax(0, 1fr);
    gap: 22px;
    align-items: start;
  }

  .section-nav {
    display: grid;
    gap: 8px;
    position: sticky;
    top: 24px;
  }

  .section-link {
    display: grid;
    min-height: 58px;
    gap: 4px;
    border-radius: 8px;
    background: transparent;
    color: #3a3f35;
    padding: 11px 12px;
    text-align: left;
  }

  .section-link.active {
    background: #e3ebe0;
    color: #162117;
  }

  .section-link span {
    font-size: 0.93rem;
    font-weight: 760;
  }

  .section-link small,
  .local-summary span,
  .recording-copy span,
  .setting-row small,
  .search-row small,
  .row-meta {
    color: #697065;
    font-size: 0.78rem;
  }

  .local-summary {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 12px;
    border-top: 1px solid #dfe5da;
    padding: 18px 4px 0;
  }

  .local-summary strong,
  .local-summary span {
    display: block;
  }

  .status-dot {
    width: 9px;
    height: 9px;
    flex: 0 0 auto;
    border-radius: 999px;
    background: #257858;
  }

  .primary-surface {
    min-width: 0;
  }

  .recording-panel,
  .queue-panel,
  .library-panel,
  .search-panel,
  .settings-panel,
  .notice {
    border: 1px solid #d9e1d5;
    border-radius: 8px;
    background: #fbfcf8;
  }

  .recording-panel,
  .queue-panel,
  .library-panel,
  .search-panel,
  .settings-panel {
    display: grid;
    gap: 20px;
    padding: 20px;
  }

  .queue-panel {
    margin-top: 18px;
  }

  .notice {
    display: grid;
    gap: 4px;
    padding: 14px 16px;
  }

  .notice.error {
    border-color: #e7b7b7;
    background: #fff7f7;
    color: #842222;
  }

  .recording-console {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) minmax(230px, 0.32fr);
    gap: 22px;
    align-items: center;
    min-height: 180px;
    border-radius: 8px;
    background:
      linear-gradient(135deg, rgba(37, 120, 88, 0.12), transparent 42%),
      #eff3ea;
    padding: 24px;
  }

  .pulse-ring {
    display: grid;
    width: 90px;
    height: 90px;
    place-items: center;
    border-radius: 999px;
    background: #dfe9dc;
  }

  .pulse-ring span {
    width: 42px;
    height: 42px;
    border-radius: 999px;
    background: #257858;
  }

  .recording-console[data-state="recording"] .pulse-ring span {
    background: #b63d34;
    animation: recording-pulse 1.4s ease-in-out infinite;
  }

  .recording-console[data-state="processing"] .pulse-ring span {
    border-radius: 8px;
    background: #b58122;
    animation: processing-rotate 1.8s linear infinite;
  }

  .recording-copy {
    display: grid;
    gap: 8px;
  }

  .recording-copy strong {
    color: #171914;
    font-size: 1.35rem;
    line-height: 1.1;
  }

  .control-stack {
    display: grid;
    gap: 12px;
  }

  .capture-config {
    display: grid;
    gap: 16px;
    border-top: 1px solid #d9e1d5;
    padding-top: 18px;
  }

  .capture-status-grid,
  .capture-controls {
    display: grid;
    gap: 12px;
  }

  .capture-status-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .capture-status-grid section {
    display: grid;
    gap: 8px;
    min-width: 0;
    border-left: 1px solid #d9e1d5;
    padding-left: 14px;
  }

  .capture-status-grid p,
  .capture-status-grid small {
    margin: 0;
    color: #5f665a;
    font-size: 0.84rem;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }

  .capture-status-grid small {
    color: #8d2727;
  }

  .capture-controls {
    grid-template-columns: repeat(2, minmax(0, 1fr)) auto;
    align-items: end;
  }

  .capture-errors {
    display: grid;
    gap: 4px;
    margin: 0;
    padding-left: 18px;
    color: #8d2727;
    font-size: 0.86rem;
    line-height: 1.45;
  }

  label {
    display: grid;
    gap: 7px;
    color: #42483d;
    font-size: 0.78rem;
    font-weight: 700;
  }

  input,
  select,
  textarea {
    min-width: 0;
    min-height: 40px;
    border: 1px solid #ccd5c8;
    border-radius: 8px;
    background: #ffffff;
    color: #171914;
    padding: 0 11px;
  }

  textarea {
    min-height: 84px;
    padding: 10px 11px;
    resize: vertical;
  }

  input:disabled,
  select:disabled,
  textarea:disabled {
    color: #697065;
  }

  .primary-action {
    min-height: 42px;
    border-radius: 8px;
    background: #257858;
    color: #ffffff;
    padding: 0 16px;
    font-weight: 780;
  }

  .primary-action.danger {
    background: #b63d34;
  }

  .primary-action:disabled,
  .ghost-button:disabled {
    cursor: default;
    opacity: 0.62;
  }

  .state-grid,
  .metadata-grid,
  .transcript-meta,
  .session-metrics,
  .settings-grid,
  .model-status-grid {
    display: grid;
    gap: 12px;
  }

  .state-grid {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .state-cell {
    display: grid;
    gap: 10px;
    border-left: 1px solid #d9e1d5;
    padding: 2px 14px;
  }

  .state-cell p,
  .player-placeholder p,
  .empty-state,
  .failure-message,
  .transcript-copy {
    margin: 0;
    color: #5f665a;
    font-size: 0.9rem;
    line-height: 1.5;
  }

  .status-chip[data-status="idle"],
  .status-chip[data-status="disabled"],
  .status-chip[data-status="pending"],
  .status-chip[data-status="not_started"],
  .status-chip[data-status="queued"],
  .status-chip[data-status="unknown"],
  .status-chip[data-status="prompt_required"] {
    background: #e7ece2;
    color: #304535;
  }

  .status-chip[data-status="recording"],
  .status-chip[data-status="capturing"],
  .status-chip[data-status="running"] {
    background: #f2dddd;
    color: #8d2727;
  }

  .status-chip[data-status="processing"] {
    background: #efe5cd;
    color: #704d10;
  }

  .status-chip[data-status="interrupted"],
  .status-chip[data-status="retrying"] {
    background: #f1e0ce;
    color: #7a3f12;
  }

  .status-chip[data-status="completed"],
  .status-chip[data-status="complete"],
  .status-chip[data-status="succeeded"] {
    background: #dcebe2;
    color: #1b694a;
  }

  .status-chip[data-status="failed"],
  .status-chip[data-status="cancelled"],
  .status-chip[data-status="denied"],
  .status-chip[data-status="unavailable"] {
    background: #f5dddd;
    color: #8d2727;
  }

  .library-layout {
    display: grid;
    grid-template-columns: minmax(230px, 0.34fr) minmax(0, 1fr);
    gap: 18px;
    align-items: start;
  }

  .recording-list,
  .search-results,
  .settings-list,
  .job-list,
  .ai-settings-block,
  .ai-output-list {
    display: grid;
    gap: 8px;
  }

  .ai-settings-grid {
    display: grid;
    grid-template-columns: minmax(0, 0.44fr) minmax(0, 1fr);
    gap: 10px;
  }

  .recording-row,
  .search-row {
    display: grid;
    width: 100%;
    gap: 6px;
    border-radius: 8px;
    background: #f1f4ed;
    color: #171914;
    padding: 12px;
    text-align: left;
  }

  .recording-row.selected,
  .search-row:hover,
  .recording-row:hover {
    background: #e3ebe0;
  }

  .row-title {
    overflow-wrap: anywhere;
    font-weight: 760;
  }

  .detail-panel {
    display: grid;
    gap: 18px;
    min-width: 0;
  }

  .detail-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    flex-wrap: wrap;
  }

  .player-surface {
    display: grid;
    gap: 10px;
  }

  .player-surface video {
    width: 100%;
    aspect-ratio: 16 / 9;
    border: 1px solid #d9e1d5;
    border-radius: 8px;
    background: #171914;
  }

  .media-path {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    color: #697065;
    font-size: 0.8rem;
  }

  .media-path strong,
  .media-path span {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .media-path strong {
    color: #343a32;
  }

  .player-placeholder {
    display: grid;
    min-height: 230px;
    place-items: center;
    border: 1px dashed #bfcabd;
    border-radius: 8px;
    background:
      linear-gradient(180deg, rgba(23, 25, 20, 0.04), transparent),
      #eff3ea;
    padding: 24px;
    text-align: center;
  }

  .player-placeholder div {
    display: grid;
    gap: 6px;
  }

  .player-placeholder strong {
    color: #171914;
    font-size: 1.15rem;
  }

  .player-placeholder span {
    color: #697065;
    overflow-wrap: anywhere;
  }

  .metadata-grid {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .transcript-meta,
  .model-status-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .session-metrics {
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border-top: 1px solid #d9e1d5;
    padding-top: 16px;
  }

  .session-detail-grid {
    border-top: 1px solid #d9e1d5;
    padding-top: 14px;
  }

  dl {
    margin: 0;
  }

  .metadata-grid div,
  .transcript-meta div,
  .session-metrics div,
  .path-list div,
  .model-status-grid div {
    min-width: 0;
  }

  dt {
    color: #697065;
    font-size: 0.74rem;
    font-weight: 720;
    text-transform: uppercase;
  }

  dd {
    margin: 4px 0 0;
    color: #171914;
    font-size: 0.88rem;
    overflow-wrap: anywhere;
  }

  .split-panels {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(210px, 0.34fr);
    gap: 18px;
  }

  .split-panels section,
  .ai-panel,
  .settings-grid section {
    display: grid;
    gap: 12px;
    border-top: 1px solid #d9e1d5;
    padding-top: 14px;
  }

  .panel-heading-inline {
    justify-content: space-between;
    gap: 12px;
  }

  .job-list,
  .ai-output-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .job-list li {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-radius: 8px;
    background: #f1f4ed;
    padding: 10px;
  }

  .job-main {
    display: grid;
    min-width: 0;
    gap: 4px;
  }

  .job-actions {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }

  .job-list span {
    overflow-wrap: anywhere;
    color: #171914;
    font-weight: 700;
  }

  .job-list small {
    color: #697065;
  }

  .job-list .job-error {
    color: #8d2727;
    overflow-wrap: anywhere;
  }

  .ai-notes {
    max-width: 720px;
  }

  .ai-output-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  .ai-output-grid section {
    display: grid;
    gap: 8px;
    min-width: 0;
    border-left: 1px solid #d9e1d5;
    padding-left: 12px;
  }

  .ai-output-grid h5 {
    margin: 0;
    color: #171914;
    font-size: 0.84rem;
    line-height: 1.2;
  }

  .ai-output-list li {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .ai-output-list strong {
    color: #343a32;
    font-size: 0.84rem;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .ai-output-list small {
    color: #697065;
    font-size: 0.74rem;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .failure-message {
    color: #8d2727;
  }

  .segment-list {
    display: grid;
    gap: 8px;
    list-style: none;
    margin: 2px 0 0;
    max-height: 360px;
    overflow: auto;
    padding: 0;
  }

  .segment-list li {
    display: grid;
    grid-template-columns: 42px minmax(0, 1fr);
    gap: 10px;
    border-radius: 8px;
    background: #f1f4ed;
    padding: 10px;
  }

  .segment-time {
    color: #697065;
    font-size: 0.76rem;
    font-weight: 760;
  }

  .segment-list p {
    display: grid;
    gap: 4px;
    margin: 0;
    color: #343a32;
    font-size: 0.86rem;
    line-height: 1.4;
  }

  .segment-list small,
  .model-status-grid small {
    color: #697065;
    font-size: 0.74rem;
    overflow-wrap: anywhere;
  }

  .search-controls {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 12px;
    align-items: end;
  }

  .search-row {
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
  }

  .search-row span:first-child {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .search-row p {
    margin: 0;
    color: #343a32;
    font-size: 0.86rem;
    line-height: 1.45;
  }

  .settings-grid {
    grid-template-columns: minmax(0, 1fr) minmax(280px, 0.42fr);
  }

  .path-list {
    display: grid;
    gap: 10px;
  }

  .setting-row {
    display: flex;
    min-height: 58px;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    border-radius: 8px;
    background: #f1f4ed;
    padding: 12px;
  }

  .setting-row span {
    display: grid;
    gap: 4px;
  }

  .setting-row select {
    width: 150px;
  }

  .setting-row input[type="checkbox"] {
    min-height: auto;
    width: 18px;
    height: 18px;
  }

  .model-status-grid {
    margin: 0;
  }

  .model-status-grid div {
    display: grid;
    gap: 8px;
    border-left: 1px solid #d9e1d5;
    padding-left: 12px;
  }

  .model-status-grid dd {
    margin-top: 0;
  }

  .import-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: end;
  }

  @keyframes recording-pulse {
    0%,
    100% {
      transform: scale(0.86);
      opacity: 0.72;
    }

    50% {
      transform: scale(1);
      opacity: 1;
    }
  }

  @keyframes processing-rotate {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 1120px) {
    .app-grid,
    .library-layout,
    .settings-grid {
      grid-template-columns: 1fr;
    }

    .section-nav {
      position: static;
      grid-template-columns: repeat(4, minmax(0, 1fr));
    }

    .local-summary {
      grid-column: 1 / -1;
      margin-top: 0;
    }
  }

  @media (max-width: 820px) {
    .workspace-header,
    .section-heading,
    .detail-header {
      align-items: flex-start;
      flex-direction: column;
    }

    .runtime-cluster {
      justify-content: flex-start;
    }

    .section-nav {
      display: flex;
      overflow-x: auto;
      padding-bottom: 4px;
    }

    .section-link {
      min-width: 150px;
    }

    .local-summary {
      min-width: 150px;
      border-top: 0;
      padding-top: 11px;
    }

    .recording-console,
    .capture-status-grid,
    .capture-controls,
    .split-panels,
    .search-controls,
    .metadata-grid,
    .transcript-meta,
    .model-status-grid,
    .ai-settings-grid,
    .ai-output-grid,
    .import-row,
    .session-metrics,
    .state-grid {
      grid-template-columns: 1fr;
    }

    .recording-console {
      min-height: auto;
      padding: 18px;
    }

    .pulse-ring {
      width: 72px;
      height: 72px;
    }

    .pulse-ring span {
      width: 34px;
      height: 34px;
    }

    .player-placeholder {
      min-height: 180px;
    }

    .media-path {
      align-items: flex-start;
      flex-direction: column;
      gap: 4px;
    }
  }
</style>
