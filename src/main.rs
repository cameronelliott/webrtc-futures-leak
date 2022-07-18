use anyhow::Result;

use std::io::BufRead;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;

use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;

use webrtc::track::track_remote::TrackRemote;

#[tokio::main]
async fn main() -> Result<()> {
    console_subscriber::init();

    let stdin = std::io::stdin();
    let mut iterator = stdin.lock().lines();
    println!("press enter to create Webrtc PCs A, B");
    let _ = iterator.next().unwrap().unwrap();

    let a_weak;
    let b_weak;
    let a_strong;
    {
        // Everything below is the WebRTC-rs API! Thanks for using it ❤️.

        // Create a MediaEngine object to configure the supported codec
        let mut m = MediaEngine::default();

        m.register_default_codecs()?;

        // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
        // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
        // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
        // for each PeerConnection.
        let mut registry = Registry::new();

        // Use the default set of Interceptors
        registry = register_default_interceptors(registry, &mut m)?;

        // Create the API object with the MediaEngine
        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        // Prepare the configuration
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create a new RTCPeerConnection
        let a = Arc::new(api.new_peer_connection(config.clone()).await?);
        let b = Arc::new(api.new_peer_connection(config).await?);

        a_strong = a.clone();

        a.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("A Peer Connection State has changed: {}", s);
            Box::pin(async {})
        }))
        .await;

        b.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("B Peer Connection State has changed: {}", s);
            Box::pin(async {})
        }))
        .await;

        // Allow us to receive 1 video track
        a.add_transceiver_from_kind(RTPCodecType::Video, &[]).await?;

        let ofr = a.create_offer(None).await?;

        // Sets the LocalDescription, and starts our UDP listeners
        a.set_local_description(ofr).await?;
        //sleep
        sleep(Duration::new(1, 0)).await;

        let ofr = a.pending_local_description().await.unwrap();
        b.set_remote_description(ofr).await?;

        let ans = b.create_answer(None).await?;

        // Sets the LocalDescription, and starts our UDP listeners

        // let mut gather_complete = a.gathering_complete_promise().await;
        b.set_local_description(ans).await?;
        //let _ = gather_complete.recv().await;
        //sleep
        sleep(Duration::new(1, 0)).await;

        let ans = b.current_local_description().await.unwrap();
        a.set_remote_description(ans).await?;
        //sleep

        // Set a handler for when a new remote track starts, this handler copies inbound RTP packets,
        // replaces the SSRC and sends them back
        a_weak = Arc::downgrade(&a);
        b_weak = Arc::downgrade(&b);

        let xx = a_weak.clone();

        a.on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
                if let Some(track) = track {
                    // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
                    // This is a temporary fix until we implement incoming RTCP events, then we would push a PLI only when a viewer requests it
                    let media_ssrc = track.ssrc();
                    let pc2 = xx.clone();
                    tokio::spawn(async move {
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(3));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() =>{
                                    if let Some(pc) = pc2.upgrade(){
                                        result = pc.write_rtcp(&[Box::new(PictureLossIndication{
                                            sender_ssrc: 0,
                                            media_ssrc,
                                        })]).await.map_err(Into::into);
                                    }else{
                                        break;
                                    }
                                }
                            };
                        }
                    });

                    // tokio::spawn(async move {
                    //     // Create Track that we send video back to browser on
                    //     let local_track = Arc::new(TrackLocalStaticRTP::new(
                    //         track.codec().await.capability,
                    //         "video".to_owned(),
                    //         "webrtc-rs".to_owned(),
                    //     ));

                    //     // Read RTP packets being sent to webrtc-rs
                    //     while let Ok((rtp, _)) = track.read_rtp().await {
                    //         if let Err(err) = local_track.write_rtp(&rtp).await {
                    //             if Error::ErrClosedPipe != err {
                    //                 print!("output track write_rtp got error: {} and break", err);
                    //                 break;
                    //             } else {
                    //                 print!("output track write_rtp got error: {}", err);
                    //             }
                    //         }
                    //     }
                    // });
                }

                Box::pin(async {})
            },
        ))
        .await;

        // println!("press enter to close A, B PCs");
        // let _ = iterator.next().unwrap().unwrap();
        // a.close().await?;
        // b.close().await?;

        println!("press to invoke Drop trait on A, B PCs");
        let _ = iterator.next().unwrap().unwrap();
    }

    println!("a_weak nweak/{} nstrong/{}", a_weak.weak_count(), a_weak.strong_count());
    println!("b_weak nweak/{} nstrong/{}", b_weak.weak_count(), b_weak.strong_count());
    drop(a_strong);
    println!("a_weak nweak/{} nstrong/{}", a_weak.weak_count(), a_weak.strong_count());
    println!("b_weak nweak/{} nstrong/{}", b_weak.weak_count(), b_weak.strong_count());

    println!("Drop on PCs invoked, sleeping forever");
    sleep(Duration::MAX).await;

    Ok(())
}
