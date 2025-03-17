package io.github.dialogflowchatbot;

import io.github.dialogflowchatbot.sdk.RequestData;
import io.github.dialogflowchatbot.sdk.Response;
import org.junit.jupiter.api.Test;

import java.io.IOException;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;

public class RequestHandlerTest {
    @Test
    void reqTest() throws IOException, InterruptedException {
        RequestData requestData = RequestData.create("r03dbzxp6zpk9uhkgcbw1ec604", "103dbzxp74kjwb148ubfmhgemb");
        RequestHandler requestHandler = new RequestHandler("http://127.0.0.1:12715/flow/answer");
        Response res = requestHandler.req(requestData, 1000);
        assertNotNull(res);
        assertNotNull(res.getData());
        assertEquals(1, res.getData().getAnswers().size());
    }
}
