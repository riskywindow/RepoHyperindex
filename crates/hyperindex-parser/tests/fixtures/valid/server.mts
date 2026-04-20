export async function bootServer() {
  const runtime = await import("./runtime.js");
  return runtime.start();
}
