type ErrorDialogProps = {
  title?: string;
  message: string;
  onClose: () => void;
};

const overlayClass = "fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4";
const panelClass = "w-full max-w-sm rounded-2xl bg-white p-5 shadow-xl";
const buttonClass = "cursor-pointer rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white hover:bg-zinc-800";

export function ErrorDialog({
  title = "Error",
  message,
  onClose,
}: ErrorDialogProps) {
  return (
    <div className={overlayClass}>
      <div className={panelClass}>
        <h2 className="text-lg font-semibold text-zinc-900">{title}</h2>
        <p className="mt-2 text-sm text-zinc-600">{message}</p>
        <div className="mt-6 flex justify-end">
          <button
            type="button"
            className={buttonClass}
            onClick={onClose}
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}
