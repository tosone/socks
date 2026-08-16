type ErrorDialogProps = {
  title?: string;
  message: string;
  onClose: () => void;
};

export function ErrorDialog({
  title = "Error",
  message,
  onClose,
}: ErrorDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-sm rounded-2xl bg-white p-5 shadow-xl">
        <h2 className="text-lg font-semibold text-zinc-900">{title}</h2>
        <p className="mt-2 text-sm text-zinc-600">{message}</p>
        <div className="mt-6 flex justify-end">
          <button
            type="button"
            className="cursor-pointer rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white hover:bg-zinc-800"
            onClick={onClose}
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}
