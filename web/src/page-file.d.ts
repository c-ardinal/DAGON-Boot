// The files the Console asks the page's own server for: importing one with
// `?url` gives its address as a PageFile, the only thing requestFile() takes.
declare module '*.dgb?url' {
  const file: import('./request.ts').PageFile;
  export default file;
}

declare module '*.img?url' {
  const file: import('./request.ts').PageFile;
  export default file;
}
