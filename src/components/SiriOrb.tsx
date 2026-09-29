import { useMemo } from "react";

export type AssistantState = "idle" | "listening" | "thinking" | "speaking";

interface SiriOrbProps {
  state: AssistantState;
  size?: "sm" | "md" | "lg";
  onClick?: () => void;
}

export default function SiriOrb({ state, size = "md", onClick }: SiriOrbProps) {
  const dimensions = useMemo(() => {
    switch (size) {
      case "sm":
        return { container: "w-8 h-8", orb: "w-6 h-6", blur: "blur-md" };
      case "lg":
        return { container: "w-28 h-28", orb: "w-24 h-24", blur: "blur-2xl" };
      case "md":
      default:
        return { container: "w-14 h-14", orb: "w-12 h-12", blur: "blur-lg" };
    }
  }, [size]);

  // Color schemes depending on state (Luxury Champagne Amber & Siri/Whisper palette)
  const auraGradients = useMemo(() => {
    switch (state) {
      case "listening":
        return "from-rose-500 via-amber-400 to-amber-600";
      case "thinking":
        return "from-amber-300 via-violet-500 to-amber-500";
      case "speaking":
        return "from-amber-400 via-orange-500 to-yellow-300";
      case "idle":
      default:
        return "from-amber-400 via-amber-600 to-yellow-200";
    }
  }, [state]);

  return (
    <div
      onClick={onClick}
      className={`relative flex items-center justify-center cursor-pointer select-none ${dimensions.container}`}
      title={
        state === "listening"
          ? "音声を聞き取り中…"
          : state === "thinking"
          ? "思考中…"
          : state === "speaking"
          ? "話しています"
          : "Veyra (クリックまたは長押しで話しかける)"
      }
    >
      {/* Background ambient glow aura */}
      <div
        className={`absolute inset-0 rounded-full bg-gradient-to-tr ${auraGradients} opacity-70 ${
          dimensions.blur
        } transition-all duration-700 ${
          state === "listening"
            ? "scale-150 animate-siri-pulse"
            : state === "thinking"
            ? "scale-125 animate-siri-rotate"
            : "scale-100 group-hover:scale-110"
        }`}
      />

      {/* Rotating iridescent core */}
      <div
        className={`relative rounded-full bg-gradient-to-r ${auraGradients} p-[1.5px] transition-transform duration-500 ${
          dimensions.orb
        } ${state === "listening" || state === "thinking" ? "animate-siri-rotate" : ""}`}
      >
        <div className="w-full h-full rounded-full bg-zinc-950/80 backdrop-blur-md flex items-center justify-center overflow-hidden">
          {/* Internal fluid light reflections */}
          <div
            className={`w-3/4 h-3/4 rounded-full bg-gradient-to-tr ${auraGradients} opacity-60 mix-blend-screen filter blur-sm transition-all duration-500 ${
              state === "listening" ? "scale-125" : "scale-90"
            }`}
          />

          {/* Center icon / equalizer wave */}
          <div className="absolute inset-0 flex items-center justify-center gap-[2.5px] z-10">
            {state === "listening" ? (
              // Whisper-style dynamic sound wave bars
              <>
                <span className="w-1 bg-white rounded-full h-3 animate-[whisper-wave_0.8s_ease-in-out_infinite]" />
                <span className="w-1 bg-white rounded-full h-5 animate-[whisper-wave_0.7s_ease-in-out_0.15s_infinite]" />
                <span className="w-1 bg-white rounded-full h-6 animate-[whisper-wave_0.9s_ease-in-out_0.3s_infinite]" />
                <span className="w-1 bg-white rounded-full h-4 animate-[whisper-wave_0.6s_ease-in-out_0.2s_infinite]" />
                <span className="w-1 bg-white rounded-full h-2 animate-[whisper-wave_0.75s_ease-in-out_0.4s_infinite]" />
              </>
            ) : state === "thinking" ? (
              // Thinking dots
              <span className="flex gap-1">
                <span className="w-1.5 h-1.5 rounded-full bg-white/90 animate-bounce" />
                <span className="w-1.5 h-1.5 rounded-full bg-white/90 animate-bounce [animation-delay:0.15s]" />
                <span className="w-1.5 h-1.5 rounded-full bg-white/90 animate-bounce [animation-delay:0.3s]" />
              </span>
            ) : state === "speaking" ? (
              // Speaking ripple icon
              <span className="text-white text-xs drop-shadow-sm">🔊</span>
            ) : (
              // Idle microphone
              <span className="text-white/80 text-xs drop-shadow-sm font-semibold">🎙</span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
