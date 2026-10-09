// Relay only the T3 server's container-loopback endpoint using the fixed image Node.
const net = require('node:net');

function forwardTcp(input, output, port = 3773) {
  const socket = net.createConnection({ host: '127.0.0.1', port, allowHalfOpen: true });
  input.on('error', error => socket.destroy(error));
  output.on('error', error => socket.destroy(error));
  input.once('close', () => {
    if (!input.readableEnded) socket.destroy();
  });
  socket.once('close', () => {
    input.unpipe(socket);
    input.destroy();
    output.end();
  });
  // Node streams bound buffering and propagate stdin EOF as a TCP half-close.
  input.pipe(socket);
  socket.pipe(output);
  return socket;
}

module.exports = { forwardTcp };
if (require.main === module) {
  // Own the stdout pipe so server EOF closes it independently of stdin.
  // Socket streams handle nonblocking pipe backpressure on Node 22 and 24;
  // file streams can fail with EAGAIN on this descriptor.
  const output = new net.Socket({ fd: 1, readable: false, writable: true });
  forwardTcp(process.stdin, output).on('error', error => {
    console.error(`AGS T3 forwarding failed: ${error.message}`);
    process.exitCode = 1;
  });
}
