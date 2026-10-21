use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct Duplex<R, W> {
    pub reader: R,
    pub writer: Option<W>,
}
impl<R: AsyncRead + Unpin, W: Unpin> AsyncRead for Duplex<R, W> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buffer)
    }
}
impl<R: Unpin, W: AsyncWrite + Unpin> AsyncWrite for Duplex<R, W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.writer.as_mut() {
            Some(writer) => Pin::new(writer).poll_write(cx, bytes),
            None => Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe))),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.writer.as_mut() {
            Some(writer) => Pin::new(writer).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(writer) = self.writer.as_mut() {
            match Pin::new(writer).poll_flush(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => {}
            }
        }
        // Child stdin is a pipe: dropping its write end is what delivers EOF.
        self.writer = None;
        Poll::Ready(Ok(()))
    }
}
